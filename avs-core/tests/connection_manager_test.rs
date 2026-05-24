use agentverse::memory::{Message, MessageRole};
use agentverse::{ConnectionManager, GenerateRequest};
use httpmock::prelude::*;

fn user_msg(s: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: s.to_string(),
    }
}

fn anthropic_ok_body() -> serde_json::Value {
    serde_json::json!({
        "content": [{"type": "text", "text": "Hi there!"}],
        "usage": {
            "input_tokens": 10, "output_tokens": 3,
            "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0
        }
    })
}

#[tokio::test]
async fn generate_succeeds_on_200() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
        then.status(200).json_body(anthropic_ok_body());
    });
    let cm = ConnectionManager::anthropic(
        &server.base_url(),
        "claude-3-5-sonnet-20241022",
        "test-key",
    );
    let result = cm
        .generate(GenerateRequest {
            system: None,
            messages: vec![user_msg("hello")],
            tools: None,
        })
        .await;
    assert!(result.is_ok(), "got error: {:?}", result.err());
    assert_eq!(result.unwrap().content, "Hi there!");
}

#[tokio::test]
async fn generate_retries_on_429_then_fails() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
        then.status(429).body("rate limited");
    });
    let cm =
        ConnectionManager::anthropic(&server.base_url(), "claude-3-5-sonnet-20241022", "test-key")
            .with_retries(1, 1); // only 1 retry, 1ms delay for test speed
    let result = cm
        .generate(GenerateRequest {
            system: None,
            messages: vec![user_msg("hello")],
            tools: None,
        })
        .await;
    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.to_lowercase().contains("rate"),
        "expected rate limit error, got: {}",
        err_str
    );
}

#[tokio::test]
async fn circuit_breaker_opens_after_failures() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path("/v1/messages");
        then.status(500).body("server error");
    });
    let cm =
        ConnectionManager::anthropic(&server.base_url(), "claude-3-5-sonnet-20241022", "test-key")
            .with_circuit_breaker(2, 30)
            .with_retries(0, 1); // no retries, instant failures
    let req =
        || GenerateRequest { system: None, messages: vec![user_msg("hi")], tools: None };
    let _ = cm.generate(req()).await;
    let _ = cm.generate(req()).await;
    let result = cm.generate(req()).await;
    assert!(
        matches!(result, Err(agentverse::ModelError::CircuitOpen(_))),
        "expected CircuitOpen, got: {:?}",
        result
    );
}

#[tokio::test]
async fn generate_sends_anthropic_headers() {
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method("POST")
            .path("/v1/messages")
            .header("x-api-key", "test-key")
            .header("anthropic-version", "2023-06-01");
        then.status(200).json_body(anthropic_ok_body());
    });
    let cm = ConnectionManager::anthropic(
        &server.base_url(),
        "claude-3-5-sonnet-20241022",
        "test-key",
    );
    let _ = cm
        .generate(GenerateRequest {
            system: None,
            messages: vec![user_msg("hi")],
            tools: None,
        })
        .await;
    m.assert();
}
