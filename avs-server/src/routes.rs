// avs-server/src/routes.rs
use crate::envelope::{Envelope, EnvelopeKind};
use agentverse::Agent;
use agentverse_guardrails::{check_output, check_prompt, RateLimiter};
use agentverse_tools::ToolRegistry;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

#[derive(Debug, Deserialize)]
pub struct InvokeRequest {
    pub user_id: String,
    pub message: String,
}

#[derive(Clone)]
pub struct AppState {
    pub agent: Arc<Mutex<Agent>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub guardrails_enabled: bool,
    pub model_name: String,
    /// Tool registry — wired but not yet consumed by invoke().
    /// To activate: pass tools to Agent::invoke() or execute via ToolRegistry.
    #[allow(dead_code)]
    pub tools: Arc<Mutex<ToolRegistry>>,
}

pub async fn invoke(
    State(state): State<AppState>,
    Json(request): Json<InvokeRequest>,
) -> impl IntoResponse {
    if request.message.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "message must not be empty"
            })),
        );
    }

    let request_id = uuid::Uuid::new_v4();
    let start = std::time::Instant::now();

    info!(
        request_id = %request_id,
        user_id = %request.user_id,
        message_len = request.message.len(),
        "Processing request"
    );

    if let Err(e) = state.rate_limiter.check(&request.user_id) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": e.to_string()
            })),
        );
    }

    if state.guardrails_enabled {
        if let Err(e) = check_prompt(&request.message) {
            error!(
                request_id = %request_id,
                error = %e,
                user_id = %request.user_id,
                "Prompt guardrail triggered"
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": e.to_string()
                })),
            );
        }
    }

    let agent = state.agent.lock().await;
    let result = agent.invoke(&request.user_id, &request.message).await;
    drop(agent);

    match result {
        Ok(response) => {
            if state.guardrails_enabled {
                if let Err(e) = check_output(&response) {
                    error!(
                        request_id = %request_id,
                        error = %e,
                        user_id = %request.user_id,
                        "Output guardrail triggered"
                    );
                    return (
                        StatusCode::FORBIDDEN,
                        Json(serde_json::json!({
                            "error": e.to_string()
                        })),
                    );
                }
            }

            info!(
                request_id = %request_id,
                user_id = %request.user_id,
                status = 200,
                latency_ms = start.elapsed().as_millis(),
                "Request completed"
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "message": response,
                    "user_id": request.user_id,
                })),
            )
        }
        Err(e) => {
            error!(
                request_id = %request_id,
                user_id = %request.user_id,
                status = 500,
                latency_ms = start.elapsed().as_millis(),
                error = %e,
                "Request failed"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": e.to_string()
                })),
            )
        }
    }
}

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "healthy",
            "model": state.model_name,
        })),
    )
}

pub async fn ready() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "ready" })),
    )
}

pub async fn aether_invoke(
    State(state): State<AppState>,
    Json(env): Json<Envelope>,
) -> impl IntoResponse {
    if env.kind != EnvelopeKind::Invoke {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "expected envelope kind: invoke" })),
        );
    }

    let input = env.payload["input"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    let agent = state.agent.lock().await;
    let result = agent.invoke("aether", &input).await;
    drop(agent);

    match result {
        Ok(output) => {
            let response = Envelope {
                id: env.id,
                kind: EnvelopeKind::Result,
                payload: serde_json::json!({ "output": output }),
                metadata: env.metadata,
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
        }
        Err(e) => {
            let response = Envelope {
                id: env.id,
                kind: EnvelopeKind::Error,
                payload: serde_json::json!({ "error": e.to_string() }),
                metadata: env.metadata,
            };
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::to_value(response).unwrap()),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::Config;
    use agentverse_tools::ToolRegistry;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::{get, post};
    use axum::Router;
    use serde_json::json;
    use tower::ServiceExt;

    fn make_state() -> AppState {
        AppState {
            agent: Arc::new(Mutex::new(
                Agent::from_config(Config {
                    provider: agentverse::ProviderConfig::OpenAI {
                        model_name: "test-model".to_string(),
                        api_key: "test-key".to_string(),
                        base_url: None,
                    },
                    max_messages: 10,
                    tools: vec![],
                    prompts_dir: None,
                    system_prompt: None,
                })
                .unwrap(),
            )),
            rate_limiter: Arc::new(RateLimiter::new(1000, 60)),
            guardrails_enabled: false,
            model_name: "test-model".to_string(),
            tools: Arc::new(Mutex::new(ToolRegistry::new())),
        }
    }

    fn make_app(state: AppState) -> Router {
        Router::new()
            .route("/health", get(health))
            .route("/ready", get(ready))
            .route("/invoke", post(invoke))
            .route("/aether/invoke", post(aether_invoke))
            .with_state(state)
    }

    async fn get_response(app: Router, path: &str) -> axum::http::Response<Body> {
        let req = Request::get(path).body(Body::empty()).unwrap();
        app.oneshot(req).await.unwrap()
    }

    async fn post_json(
        app: Router,
        path: &str,
        body: serde_json::Value,
    ) -> axum::http::Response<Body> {
        let req = Request::post(path)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn test_health_returns_200() {
        let state = make_state();
        let app = make_app(state);

        let res = get_response(app, "/health").await;
        assert_eq!(res.status(), StatusCode::OK);

        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body["status"], "healthy");
        assert_eq!(body["model"], "test-model");
    }

    #[tokio::test]
    async fn test_ready_returns_200() {
        let state = make_state();
        let app = make_app(state);

        let res = get_response(app, "/ready").await;
        assert_eq!(res.status(), StatusCode::OK);

        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body["status"], "ready");
    }

    #[tokio::test]
    async fn test_invoke_empty_message_returns_400() {
        let state = make_state();
        let app = make_app(state);

        let body = json!({
            "user_id": "test-user",
            "message": ""
        });

        let res = post_json(app, "/invoke", body).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        let error_body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert!(error_body["error"].as_str().unwrap().contains("empty"));
    }

    #[tokio::test]
    async fn test_invoke_with_message_succeeds_or_fails_gracefully() {
        let state = make_state();
        let app = make_app(state);

        let body = json!({
            "user_id": "test-user",
            "message": "Hello, agent!"
        });

        let res = post_json(app, "/invoke", body).await;

        // Returns 200 if the model provider accepts it, or 500 if the API call fails
        // (which is expected since this is a test key)
        assert!(
            res.status() == StatusCode::OK || res.status() == StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn test_aether_invoke_with_invoke_kind() {
        let state = make_state();
        let app = make_app(state);
        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "kind": "invoke",
            "payload": {"input": "hello from aether"},
            "metadata": {}
        });
        let res = post_json(app, "/aether/invoke", env).await;
        // 200 (model ok) or 500 (API unreachable with test key) — not 400
        assert_ne!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_aether_invoke_non_invoke_kind_returns_400() {
        let state = make_state();
        let app = make_app(state);
        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000002",
            "kind": "ping",
            "payload": {},
            "metadata": {}
        });
        let res = post_json(app, "/aether/invoke", env).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
