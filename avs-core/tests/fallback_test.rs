use agentverse::memory::{Message, MessageRole};
use agentverse::model::{ConnectionManager, GenerateRequest};
use httpmock::prelude::*;

#[tokio::test]
async fn fallback_provider_is_tried_when_primary_circuit_open() {
    // Primary: always returns 500 to trip the circuit
    let primary_server = MockServer::start();
    primary_server.mock(|when, then| {
        when.method(POST).path("/chat/completions");
        then.status(500).body("primary down");
    });

    // Fallback: returns a valid OpenAI-compatible response
    let fallback_server = MockServer::start();
    fallback_server.mock(|when, then| {
        when.method(POST).path("/chat/completions");
        then.status(200).json_body(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "fallback reply"}}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
        }));
    });

    // Primary: 0 retries so circuit trips on first failure; circuit breaker trips after 1 failure
    let primary = ConnectionManager::openai(&primary_server.base_url(), "gpt-test", "sk-test")
        .with_retries(0, 0)
        .with_circuit_breaker(1, 0); // threshold=1, timeout=0s (immediately half-open but trip fast)

    let fallback = ConnectionManager::openai(&fallback_server.base_url(), "gpt-test", "sk-test");

    let cm = primary.with_fallback(fallback);

    let req = GenerateRequest {
        system: None,
        messages: vec![Message {
            role: MessageRole::User,
            content: "test".into(),
        }],
        tools: None,
        ..Default::default()
    };

    // First call: primary fails (500), retry-exhausted fallback path kicks in
    let _ = cm.generate(req.clone()).await;

    // Second call: primary circuit is open → circuit-open fallback path kicks in
    let resp = cm.generate(req).await.expect("fallback should succeed");
    assert_eq!(resp.content, "fallback reply");
}
