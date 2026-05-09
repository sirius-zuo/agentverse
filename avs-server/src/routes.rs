// avs-server/src/routes.rs
use agentverse::Agent;
use agentverse_guardrails::{check_output, check_prompt, RateLimiter};
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
}

pub async fn invoke(
    State(state): State<AppState>,
    Json(request): Json<InvokeRequest>,
) -> impl IntoResponse {
    // Rate limiting
    if let Err(e) = state.rate_limiter.check(&request.user_id) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": e.to_string()
            })),
        );
    }

    // Prompt guardrail
    if state.guardrails_enabled {
        if let Err(e) = check_prompt(&request.message) {
            error!(error = %e, user_id = %request.user_id, "Prompt guardrail triggered");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": e.to_string()
                })),
            );
        }
    }

    info!(user_id = %request.user_id, message = %request.message, "Processing request");

    let agent = state.agent.lock().await;
    let result = agent.invoke(&request.user_id, &request.message).await;

    drop(agent);

    match result {
        Ok(response) => {
            // Output guardrail
            if state.guardrails_enabled {
                if let Err(e) = check_output(&response) {
                    error!(error = %e, user_id = %request.user_id, "Output guardrail triggered");
                    return (
                        StatusCode::FORBIDDEN,
                        Json(serde_json::json!({
                            "error": e.to_string()
                        })),
                    );
                }
            }

            info!(user_id = %request.user_id, "Request completed");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "message": response,
                    "user_id": request.user_id,
                })),
            )
        }
        Err(e) => {
            error!(error = %e, user_id = %request.user_id, "Request failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": e.to_string()
                })),
            )
        }
    }
}

pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "healthy",
            "model": "gpt-4",
        })),
    )
}

pub async fn ready() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "ready" })),
    )
}
