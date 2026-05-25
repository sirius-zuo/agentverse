use crate::{Agent, AgentError};
use agentverse_session::SessionStoreError;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

fn store_err_status(e: &SessionStoreError) -> StatusCode {
    match e {
        SessionStoreError::NotFound(_) => StatusCode::NOT_FOUND,
        SessionStoreError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── POST /sessions ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub user_id: String,
}

pub async fn create_session(
    State(agent): State<Arc<Agent>>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    if req.user_id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "user_id must not be empty" })),
        );
    }
    match agent.create_session(&req.user_id).await {
        Ok(session_id) => {
            info!(user_id = %req.user_id, %session_id, "Session created");
            (
                StatusCode::CREATED,
                Json(serde_json::json!({ "session_id": session_id })),
            )
        }
        Err(e) => {
            error!(error = %e, "Failed to create session");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

// ── POST /sessions/:session_id/messages ───────────────────────────────────────

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub user_id: String,
    pub message: String,
}

pub async fn send_message(
    State(agent): State<Arc<Agent>>,
    Path(session_id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    if req.message.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "message must not be empty" })),
        );
    }
    match agent.invoke(&req.user_id, session_id, &req.message).await {
        Ok(reply) => {
            info!(user_id = %req.user_id, %session_id, "Message sent");
            (
                StatusCode::OK,
                Json(serde_json::json!({ "session_id": session_id, "reply": reply })),
            )
        }
        Err(e) => {
            let status = match &e {
                AgentError::Session(se) => store_err_status(se),
                AgentError::Llm(_) => StatusCode::BAD_GATEWAY,
            };
            error!(error = %e, "Failed to invoke session");
            (status, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

// ── GET /sessions/:session_id ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct GetSessionQuery {
    pub user_id: String,
}

pub async fn get_session(
    State(agent): State<Arc<Agent>>,
    Path(session_id): Path<Uuid>,
    Query(query): Query<GetSessionQuery>,
) -> impl IntoResponse {
    match agent.get_session(&query.user_id, session_id).await {
        Ok(Some(session)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "session_id": session.id,
                "user_id": session.user_id,
                "status": session.status.to_string(),
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "session not found" })),
        ),
        Err(e) => {
            let status = match &e {
                AgentError::Session(se) => store_err_status(se),
                AgentError::Llm(_) => StatusCode::BAD_GATEWAY,
            };
            error!(error = %e, "Failed to get session");
            (status, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

// ── DELETE /sessions/:session_id ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EndSessionRequest {
    pub user_id: String,
}

pub async fn end_session(
    State(agent): State<Arc<Agent>>,
    Path(session_id): Path<Uuid>,
    Json(req): Json<EndSessionRequest>,
) -> impl IntoResponse {
    match agent.end_session(&req.user_id, session_id).await {
        Ok(()) => {
            info!(user_id = %req.user_id, %session_id, "Session ended");
            (StatusCode::NO_CONTENT, Json(serde_json::json!({})))
        }
        Err(e) => {
            let status = match &e {
                AgentError::Session(se) => store_err_status(se),
                AgentError::Llm(_) => StatusCode::BAD_GATEWAY,
            };
            error!(error = %e, "Failed to end session");
            (status, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry, ProviderConfig};
    use agentverse_memory::SimpleMemory;
    use agentverse_session::SqliteSessionStore;
    use agentverse_strategy::{build as build_strategy, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use axum::routing::{delete, get, post};
    use axum::{body::Body, http::Request, Router};
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    async fn make_agent() -> Arc<Agent> {
        let store = Arc::new(SqliteSessionStore::new("sqlite::memory:").await.unwrap());
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: ProviderConfig::OpenAI {
                    model_name: "gpt-4o".to_string(),
                    api_key: "sk-test".to_string(),
                    base_url: Some("http://127.0.0.1:1/v1".to_string()),
                },
                max_messages: 50,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap(),
        );
        let tools = Arc::new(ToolRegistry::new());
        let prompts = Arc::new(PromptRegistry::new());
        let memory: Arc<Mutex<dyn agentverse::Memory>> =
            Arc::new(Mutex::new(SimpleMemory::new(50)));
        let strategy = build_strategy(
            StrategyKind::React,
            Arc::clone(&runner),
            Arc::clone(&prompts),
            Arc::clone(&tools),
            Arc::clone(&memory),
            5,
        );
        Agent::new(runner, tools, prompts, memory, store, strategy, false)
    }

    fn make_app(agent: Arc<Agent>) -> Router {
        Router::new()
            .route("/sessions", post(create_session))
            .route("/sessions/:session_id/messages", post(send_message))
            .route("/sessions/:session_id", get(get_session))
            .route("/sessions/:session_id", delete(end_session))
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

    async fn body_json(res: axum::http::Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn test_create_session_returns_201() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let res = post_json(app, "/sessions", serde_json::json!({ "user_id": "alice" })).await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = body_json(res).await;
        assert!(body["session_id"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_create_session_empty_user_returns_400() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let res = post_json(app, "/sessions", serde_json::json!({ "user_id": "" })).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_session_not_found_returns_404() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let unknown_id = Uuid::new_v4();
        let req = Request::get(format!("/sessions/{}?user_id=alice", unknown_id))
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_session_returns_200() {
        let agent = make_agent().await;
        // Create a session first
        let session_id = agent.create_session("alice").await.unwrap();
        let app = make_app(agent);
        let req = Request::get(format!("/sessions/{}?user_id=alice", session_id))
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["user_id"], "alice");
        assert_eq!(body["status"], "active");
    }

    #[tokio::test]
    async fn test_send_message_empty_message_returns_400() {
        let agent = make_agent().await;
        let session_id = agent.create_session("alice").await.unwrap();
        let app = make_app(agent);
        let res = post_json(
            app,
            &format!("/sessions/{}/messages", session_id),
            serde_json::json!({ "user_id": "alice", "message": "" }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_end_session_returns_204() {
        let agent = make_agent().await;
        let session_id = agent.create_session("alice").await.unwrap();
        let app = make_app(agent);
        let req = Request::delete(format!("/sessions/{}", session_id))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "user_id": "alice" }).to_string(),
            ))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_send_message_wrong_user_returns_404() {
        let agent = make_agent().await;
        let session_id = agent.create_session("alice").await.unwrap();
        let app = make_app(agent);
        // Bob tries to message Alice's session
        let res = post_json(
            app,
            &format!("/sessions/{}/messages", session_id),
            serde_json::json!({ "user_id": "bob", "message": "hi" }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
