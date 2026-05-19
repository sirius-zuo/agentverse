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

/// Connects to GitHub via webhooks (input) and the REST API (output).
///
/// Input: starts an axum HTTP server on `port`. Configure your GitHub repo
/// to send PR and issue events to `http://<host>:<port>/github/events`.
///
/// Output: posts a comment to the issue/PR identified by `event.conversation_id`
/// which must be in the format `"{owner}/{repo}#{number}"`.
///
/// SECURITY: incoming webhook payloads are verified with HMAC-SHA256 using
/// `webhook_secret`. Keep the secret out of source control.
pub struct GithubConnector {
    token: String,
    webhook_secret: String,
    port: u16,
    tx: mpsc::Sender<Event>,
    rx: Mutex<mpsc::Receiver<Event>>,
}

impl GithubConnector {
    pub fn new(token: &str, webhook_secret: &str, port: u16) -> Self {
        let (tx, rx) = mpsc::channel(128);
        Self {
            token: token.to_string(),
            webhook_secret: webhook_secret.to_string(),
            port,
            tx,
            rx: Mutex::new(rx),
        }
    }
}

#[derive(Clone)]
struct GithubState {
    tx: mpsc::Sender<Event>,
    webhook_secret: String,
}

#[async_trait]
impl Connector for GithubConnector {
    fn name(&self) -> &str {
        "github"
    }

    async fn start(&self) -> Result<(), ConnectorError> {
        let state = GithubState {
            tx: self.tx.clone(),
            webhook_secret: self.webhook_secret.clone(),
        };
        let port = self.port;
        tokio::spawn(async move {
            let app = Router::new()
                .route("/github/events", post(github_event_handler))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
                .await
                .expect("github: bind failed");
            axum::serve(listener, app)
                .await
                .expect("github: serve failed");
        });
        Ok(())
    }
}

#[async_trait]
impl InputConnector for GithubConnector {
    async fn receive(&self) -> Result<Event, ConnectorError> {
        let mut rx = self.rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| ConnectorError::Connection("github channel closed".to_string()))
    }
}

#[async_trait]
impl OutputConnector for GithubConnector {
    /// Posts a comment to the issue/PR in `event.conversation_id`.
    ///
    /// `conversation_id` must be `"{owner}/{repo}#{number}"`.
    async fn send(&self, event: Event) -> Result<(), ConnectorError> {
        let (repo_path, number) = parse_conversation_id(&event.conversation_id)?;
        let url = format!(
            "https://api.github.com/repos/{}/issues/{}/comments",
            repo_path, number
        );
        reqwest::Client::new()
            .post(&url)
            .bearer_auth(&self.token)
            .header("User-Agent", "agentverse-integration")
            .json(&serde_json::json!({ "body": event.text }))
            .send()
            .await
            .map_err(|e| ConnectorError::Platform(e.to_string()))?;
        Ok(())
    }
}

/// Parses `"{owner}/{repo}#{number}"` into `("{owner}/{repo}", "{number}")`.
fn parse_conversation_id(id: &str) -> Result<(&str, &str), ConnectorError> {
    id.rsplit_once('#').ok_or_else(|| {
        ConnectorError::Platform(format!(
            "invalid conversation_id '{}': expected 'owner/repo#number'",
            id
        ))
    })
}

async fn github_event_handler(
    State(state): State<GithubState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    verify_github_signature(&state.webhook_secret, &headers, &body)?;

    let payload: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let event_type = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let evt = match event_type {
        "issue_comment" | "issues" => {
            let repo = payload["repository"]["full_name"].as_str().unwrap_or("");
            let number = payload["issue"]["number"].as_u64().unwrap_or(0);
            let user = payload["sender"]["login"].as_str().unwrap_or("");
            let text = payload["comment"]["body"]
                .as_str()
                .or_else(|| payload["issue"]["body"].as_str())
                .unwrap_or("");
            Some(Event {
                id: Uuid::new_v4(),
                conversation_id: format!("{}#{}", repo, number),
                user_id: user.to_string(),
                text: text.to_string(),
                metadata: HashMap::from([("event_type".to_string(), event_type.to_string())]),
            })
        }
        "pull_request" => {
            let repo = payload["repository"]["full_name"].as_str().unwrap_or("");
            let number = payload["pull_request"]["number"].as_u64().unwrap_or(0);
            let user = payload["sender"]["login"].as_str().unwrap_or("");
            let text = payload["pull_request"]["body"].as_str().unwrap_or("");
            Some(Event {
                id: Uuid::new_v4(),
                conversation_id: format!("{}#{}", repo, number),
                user_id: user.to_string(),
                text: text.to_string(),
                metadata: HashMap::from([("event_type".to_string(), "pull_request".to_string())]),
            })
        }
        _ => None,
    };

    if let Some(e) = evt {
        let _ = state.tx.send(e).await;
    }

    Ok(axum::Json(serde_json::json!({ "ok": true })))
}

fn verify_github_signature(
    secret: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<(), StatusCode> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let sig_header = headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    mac.update(body);
    let computed = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

    if computed != sig_header {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}
