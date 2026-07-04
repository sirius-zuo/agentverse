use crate::Agent;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Debug, Deserialize)]
pub struct InvokeRequest {
    pub user_id: String,
    pub message: String,
}

pub async fn invoke(
    State(agent): State<Arc<Agent>>,
    Extension(limiter): Extension<Arc<agentverse_guardrails::RateLimiter>>,
    Json(request): Json<InvokeRequest>,
) -> impl IntoResponse {
    if let Err(e) = limiter.check(&request.user_id) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": e.to_string() })),
        );
    }
    if request.message.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "message must not be empty" })),
        );
    }

    match agent.invoke_stateless(&request.message).await {
        Ok(reply) => {
            info!(user_id = %request.user_id, "Invoke completed");
            (
                StatusCode::OK,
                Json(serde_json::json!({ "message": reply, "user_id": request.user_id })),
            )
        }
        Err(e) => {
            error!(error = %e, user_id = %request.user_id, "Invoke failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "healthy" })),
    )
}

pub async fn ready() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "ready" })),
    )
}

pub async fn aether_invoke(
    State(agent): State<Arc<Agent>>,
    Json(env): Json<super::envelope::Envelope>,
) -> impl IntoResponse {
    use super::envelope::EnvelopeKind;

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

    match agent.invoke_stateless(&input).await {
        Ok(reply) => {
            let response = super::envelope::Envelope {
                id: env.id,
                kind: EnvelopeKind::Result,
                payload: serde_json::json!({ "output": reply }),
                metadata: env.metadata,
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
        }
        Err(e) => {
            let response = super::envelope::Envelope {
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
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{get, post};
    use axum::Router;
    use tower::ServiceExt;

    async fn make_agent() -> Arc<Agent> {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::openai(
                    "test".to_string(),
                    "sk-test".to_string(),
                    Some("http://127.0.0.1:1/v1".to_string()),
                ),
                max_messages: 10,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap(),
        );
        let tools = ToolRegistry::new();
        let prompts = Arc::new(PromptRegistry::new());
        let strategy = build(
            StrategyKind::React,
            Arc::clone(&runner),
            Arc::clone(&prompts),
            Arc::clone(&tools),
            3,
        );
        let session_memory = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
        Agent::builder(runner, tools, prompts, session_memory, strategy).build()
    }

    fn make_app(agent: Arc<Agent>) -> Router {
        let limiter = Arc::new(agentverse_guardrails::RateLimiter::new(1000, 60));
        Router::new()
            .route("/health", get(health))
            .route("/ready", get(ready))
            .route("/invoke", post(invoke))
            .route("/aether/invoke", post(aether_invoke))
            .layer(axum::Extension(limiter))
            .with_state(agent)
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
        let agent = make_agent().await;
        let app = make_app(agent);
        let req = Request::get("/health").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body["status"], "healthy");
    }

    #[tokio::test]
    async fn test_ready_returns_200() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let req = Request::get("/ready").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body["status"], "ready");
    }

    #[tokio::test]
    async fn test_invoke_empty_message_returns_400() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let res = post_json(
            app,
            "/invoke",
            serde_json::json!({"user_id": "test", "message": ""}),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert!(body["error"].as_str().unwrap().contains("empty"));
    }

    #[tokio::test]
    async fn test_invoke_with_message_fails_gracefully() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let res = post_json(
            app,
            "/invoke",
            serde_json::json!({"user_id": "test", "message": "Hello, agent!"}),
        )
        .await;
        // 200 if model provider accepts, 500 if unreachable (expected with test key)
        assert!(
            res.status() == StatusCode::OK || res.status() == StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn test_aether_invoke_non_invoke_kind_returns_400() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000002",
            "kind": "ping",
            "payload": {},
            "metadata": {}
        });
        let res = post_json(app, "/aether/invoke", env).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_aether_invoke_with_invoke_kind() {
        let agent = make_agent().await;
        let app = make_app(agent);
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

    fn make_versioned_app(agent: Arc<Agent>) -> Router {
        let limiter = Arc::new(agentverse_guardrails::RateLimiter::new(1000, 60));
        Router::new()
            .route("/v1/health", get(health))
            .route("/v1/ready", get(ready))
            .route("/v1/invoke", post(invoke))
            .route("/openapi.json", get(super::super::openapi::openapi_json))
            .layer(axum::Extension(limiter))
            .with_state(agent)
    }

    #[tokio::test]
    async fn v1_health_returns_200() {
        let agent = make_agent().await;
        let app = make_versioned_app(agent);
        let req = Request::get("/v1/health").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn openapi_json_returns_200() {
        let agent = make_agent().await;
        let app = make_versioned_app(agent);
        let req = Request::get("/openapi.json").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body["openapi"], "3.1.0");
    }

    #[tokio::test]
    async fn test_rate_limited_invoke_returns_429() {
        let agent = make_agent().await;
        let limiter = Arc::new(agentverse_guardrails::RateLimiter::new(0, 60));
        let app = Router::new()
            .route("/invoke", post(invoke))
            .layer(axum::Extension(limiter))
            .with_state(agent);
        let res = post_json(
            app,
            "/invoke",
            serde_json::json!({"user_id": "test", "message": "Hello"}),
        )
        .await;
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
