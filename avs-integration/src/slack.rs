use super::connector::{Connector, InputConnector, OutputConnector};
use super::error::ConnectorError;
use super::event::Event;
use async_trait::async_trait;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use bytes::Bytes;
use std::collections::HashMap;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

/// Connects to Slack via the Events API (input) and Web API (output).
///
/// Input: starts an axum HTTP server on `port`. Slack must be configured
/// to POST events to `http://<host>:<port>/slack/events`.
///
/// Output: calls `chat.postMessage` with `event.conversation_id` as channel.
///
/// SECURITY: incoming requests are verified with HMAC-SHA256 using
/// `signing_secret`. Keep the signing secret out of source control.
pub struct SlackConnector {
    bot_token: String,
    signing_secret: String,
    port: u16,
    tx: mpsc::Sender<Event>,
    rx: Mutex<mpsc::Receiver<Event>>,
}

impl SlackConnector {
    pub fn new(bot_token: &str, signing_secret: &str, port: u16) -> Self {
        let (tx, rx) = mpsc::channel(128);
        Self {
            bot_token: bot_token.to_string(),
            signing_secret: signing_secret.to_string(),
            port,
            tx,
            rx: Mutex::new(rx),
        }
    }
}

#[derive(Clone)]
struct SlackState {
    tx: mpsc::Sender<Event>,
    signing_secret: String,
}

#[async_trait]
impl Connector for SlackConnector {
    fn name(&self) -> &str {
        "slack"
    }

    /// Starts the axum webhook listener in a background task.
    async fn start(&self) -> Result<(), ConnectorError> {
        let state = SlackState {
            tx: self.tx.clone(),
            signing_secret: self.signing_secret.clone(),
        };
        let port = self.port;
        tokio::spawn(async move {
            let app = Router::new()
                .route("/slack/events", post(slack_event_handler))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
                .await
                .expect("slack: bind failed");
            axum::serve(listener, app)
                .await
                .expect("slack: serve failed");
        });
        Ok(())
    }
}

#[async_trait]
impl InputConnector for SlackConnector {
    async fn receive(&self) -> Result<Event, ConnectorError> {
        let mut rx = self.rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| ConnectorError::Connection("slack channel closed".to_string()))
    }
}

#[async_trait]
impl OutputConnector for SlackConnector {
    async fn send(&self, event: Event) -> Result<(), ConnectorError> {
        let client = reqwest::Client::new();
        client
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.bot_token)
            .json(&serde_json::json!({
                "channel": event.conversation_id,
                "text": event.text,
            }))
            .send()
            .await
            .map_err(|e| ConnectorError::Platform(e.to_string()))?;
        Ok(())
    }
}

async fn slack_event_handler(
    State(state): State<SlackState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    verify_slack_signature(&state.signing_secret, &headers, &body)?;

    let payload: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Respond to Slack's URL verification challenge.
    if payload["type"] == "url_verification" {
        return Ok(axum::Json(
            serde_json::json!({ "challenge": payload["challenge"] }),
        ));
    }

    if let Some(event) = payload.get("event") {
        if event["type"] == "message" && event.get("bot_id").is_none() {
            let evt = Event {
                id: Uuid::new_v4(),
                conversation_id: event["channel"].as_str().unwrap_or("").to_string(),
                user_id: event["user"].as_str().unwrap_or("").to_string(),
                text: event["text"].as_str().unwrap_or("").to_string(),
                metadata: HashMap::new(),
            };
            let _ = state.tx.send(evt).await;
        }
    }

    Ok(axum::Json(serde_json::json!({ "ok": true })))
}

fn verify_slack_signature(
    secret: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<(), StatusCode> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let timestamp = headers
        .get("X-Slack-Request-Timestamp")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let sig_header = headers
        .get("X-Slack-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let basestring = format!("v0:{}:{}", timestamp, String::from_utf8_lossy(body));
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    mac.update(basestring.as_bytes());
    let computed = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

    if computed != sig_header {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}
