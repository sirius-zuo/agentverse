use agentverse::{Agent, Config, ProviderConfig};
use agentverse_integration::{
    IntegrationAdapter, SlackAdapter, WebhookAdapter, WebhookRequest, WebhookResponse,
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Helper to create a test agent with a minimal config.
fn test_agent() -> Arc<Mutex<Agent>> {
    let config = Config {
        provider: ProviderConfig::OpenAI {
            model_name: "test-model".to_string(),
            api_key: "test-key".to_string(),
            base_url: None,
        },
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };
    let agent = Agent::from_config(config).expect("should create test agent");
    Arc::new(Mutex::new(agent))
}

#[test]
fn test_webhook_adapter_name() {
    let adapter = WebhookAdapter::new(test_agent(), 8080, None);
    assert_eq!(adapter.name(), "webhook");
}

#[test]
fn test_slack_adapter_name() {
    let adapter = SlackAdapter::new(test_agent(), "xoxb-test", "secret", 3000);
    assert_eq!(adapter.name(), "slack");
}

#[tokio::test]
async fn test_webhook_adapter_health_check() {
    let adapter = WebhookAdapter::new(test_agent(), 8080, None);
    assert!(adapter.health_check().await.is_ok());
}

#[tokio::test]
async fn test_slack_adapter_health_check() {
    let adapter = SlackAdapter::new(test_agent(), "xoxb-test", "secret", 3000);
    assert!(adapter.health_check().await.is_ok());
}

#[tokio::test]
async fn test_webhook_adapter_stop_without_start() {
    // Stopping an adapter that was never started should be a no-op.
    let adapter = WebhookAdapter::new(test_agent(), 8080, None);
    adapter.stop().await; // Should not panic
}

/// Test the webhook handler endpoint with a real Agent.
#[tokio::test]
async fn test_webhook_endpoint() {
    let agent = test_agent();
    let port = 18923;

    let adapter = WebhookAdapter::new(Arc::clone(&agent), port, None);

    use agentverse_integration::webhook::{handle_webhook, WebhookState};
    use axum::routing::post;

    let app = Router::new()
        .route("/webhook", post(handle_webhook))
        .with_state(WebhookState {
            agent: Arc::clone(&agent),
            auth_token: None,
        });

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .expect("bind should succeed");
    let server_addr = listener.local_addr().expect("should have local addr");

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/webhook", server_addr.port());
    let resp = client
        .post(&url)
        .json(&json!({
            "user_id": "test-user",
            "message": "hello"
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);

    let body: WebhookResponse = resp.json().await.expect("should parse JSON");
    assert!(body.message.contains("hello"));

    adapter.stop().await;
    server.abort();
}

/// Test webhook with auth token configured (simplified — token not actually enforced).
#[tokio::test]
async fn test_webhook_with_auth_token() {
    let agent = test_agent();
    let port = 18924;

    let adapter = WebhookAdapter::new(Arc::clone(&agent), port, Some("secret-token".to_string()));

    use agentverse_integration::webhook::{handle_webhook, WebhookState};
    use axum::routing::post;

    let app = Router::new()
        .route("/webhook", post(handle_webhook))
        .with_state(WebhookState {
            agent: Arc::clone(&agent),
            auth_token: Some("secret-token".to_string()),
        });

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .expect("bind should succeed");
    let server_addr = listener.local_addr().expect("should have local addr");

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/webhook", server_addr.port());
    let resp = client
        .post(&url)
        .json(&json!({
            "user_id": "auth-user",
            "message": "authenticated"
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);

    adapter.stop().await;
    server.abort();
}

#[test]
fn test_webhook_request_serialization() {
    let json = r#"{"user_id":"u1","message":"hi"}"#;
    let req: WebhookRequest = serde_json::from_str(json).expect("should deserialize");
    assert_eq!(req.user_id, "u1");
    assert_eq!(req.message, "hi");
}

#[test]
fn test_webhook_response_serialization() {
    let resp = WebhookResponse {
        message: "ok".to_string(),
    };
    let json = serde_json::to_string(&resp).expect("should serialize");
    assert!(json.contains("\"message\":\"ok\""));
}

use axum::http::StatusCode;
use axum::Router;
