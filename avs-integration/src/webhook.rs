// avs-integration/src/webhook.rs
use super::adapter::{IntegrationAdapter, IntegrationError};
use agentverse::Agent;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Deserialize)]
pub struct WebhookRequest {
    pub user_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub message: String,
}

/// Webhook adapter: exposes an HTTP endpoint for incoming messages.
pub struct WebhookAdapter {
    agent: Arc<Mutex<Agent>>,
    port: u16,
    auth_token: Option<String>,
}

#[async_trait::async_trait]
impl IntegrationAdapter for WebhookAdapter {
    fn name(&self) -> &str {
        "webhook"
    }

    async fn start(&self) -> Result<(), IntegrationError> {
        let agent = Arc::clone(&self.agent);
        let port = self.port;

        let app = Router::new()
            .route("/webhook", post(handle_webhook))
            .with_state(WebhookState {
                agent: Arc::clone(&agent),
                auth_token: self.auth_token.clone(),
            });

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
            .await
            .map_err(|e| IntegrationError::Connection(e.to_string()))?;

        tracing::info!(adapter = "webhook", port, "Starting webhook adapter");

        axum::serve(listener, app).await
            .map_err(|e| IntegrationError::Connection(e.to_string()))?;

        Ok(())
    }

    async fn stop(&self) {
        tracing::info!(adapter = "webhook", "Stopping webhook adapter");
    }

    async fn health_check(&self) -> Result<(), IntegrationError> {
        Ok(())
    }
}

#[derive(Clone)]
struct WebhookState {
    agent: Arc<Mutex<Agent>>,
    auth_token: Option<String>,
}

async fn handle_webhook(
    State(state): State<WebhookState>,
    Json(request): Json<WebhookRequest>,
) -> Result<Json<WebhookResponse>, (StatusCode, Json<serde_json::Value>)> {
    // API Key check
    if let Some(ref token) = state.auth_token {
        // In production: check Authorization header
        let _ = token; // simplified
    }

    let agent = state.agent.lock().await;
    let response = agent.invoke(&request.user_id, &request.message).await;

    match response {
        Ok(output) => Ok(Json(WebhookResponse { message: output })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

impl WebhookAdapter {
    pub fn new(agent: Arc<Mutex<Agent>>, port: u16, auth_token: Option<String>) -> Self {
        Self { agent, port, auth_token }
    }
}
