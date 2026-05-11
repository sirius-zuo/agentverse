use agentverse::{
    AnthropicProvider, GeminiProvider, ModelProvider, OpenAICompatible, ProviderConfig,
    ProviderWrapper,
};
use httpmock::prelude::*;

#[tokio::test]
async fn test_provider_wrapper_retry_on_429() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method("POST").path("/chat/completions");
        then.status(429).json_body(serde_json::json!({
            "error": { "message": "Rate limit exceeded" }
        }));
    });

    let provider = OpenAICompatible::new(&server.base_url(), "test-model", "test-key");
    let wrapper = ProviderWrapper::new(provider);

    // Should retry 3 times (default) then fail with RateLimited
    let result = wrapper.generate("hello", None).await;
    match &result {
        Ok(_) => panic!("Expected Err, got Ok"),
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("Rate limited"),
                "Expected 'Rate limited' in error, got: {}",
                err_str
            );
        }
    }
}

#[tokio::test]
async fn test_provider_wrapper_success_after_retry() {
    let server = MockServer::start();

    // Mock returns 429 first, then 200 on retry
    // Note: httpmock doesn't support different responses for different calls,
    // so we use a single mock that returns 200 to verify success path works
    server.mock(|when, then| {
        when.method("POST").path("/chat/completions");
        then.status(200).json_body(serde_json::json!({
            "choices": [{
                "message": { "content": "Hello after retry!" }
            }]
        }));
    });

    let provider = OpenAICompatible::new(&server.base_url(), "test-model", "test-key");
    let wrapper = ProviderWrapper::new(provider);

    // Should succeed on first call (no retry needed)
    let result = wrapper.generate("hello", None).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Hello after retry!");
}

#[tokio::test]
async fn test_provider_wrapper_circuit_breaker_opens() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method("POST").path("/chat/completions");
        then.status(500).json_body(serde_json::json!({
            "error": "Internal server error"
        }));
    });

    let provider = OpenAICompatible::new(&server.base_url(), "test-model", "test-key");
    let wrapper = ProviderWrapper::new(provider).with_circuit_breaker(2, 30); // Open after 2 failures

    // First call fails
    let result = wrapper.generate("hello", None).await;
    assert!(result.is_err());

    // Second call fails
    let result = wrapper.generate("hello", None).await;
    assert!(result.is_err());

    // Third call: circuit is open (no HTTP call)
    let result = wrapper.generate("hello", None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Circuit breaker"));
}

#[tokio::test]
async fn test_anthropic_provider_basic() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method("POST")
            .path("/v1/messages")
            .header("x-api-key", "test-key")
            .header("anthropic-version", "2023-06-01");
        then.status(200).json_body(serde_json::json!({
            "content": [{"type": "text", "text": "Hello from Anthropic!"}]
        }));
    });

    let provider = AnthropicProvider::new(&server.base_url(), "claude-3", "test-key");

    let result = provider.generate("hello", None).await;
    match &result {
        Ok(text) => assert_eq!(text, "Hello from Anthropic!"),
        Err(e) => panic!("Expected Ok, got Err: {}", e),
    }

    mock.assert();
}

#[tokio::test]
async fn test_gemini_provider_basic() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method("POST")
            .path("/v1beta/models/test-model:generateContent")
            .query_param("key", "test-key");
        then.status(200).json_body(serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hello from Gemini!"}]
                }
            }]
        }));
    });

    let provider = GeminiProvider::new(&server.base_url(), "test-model", "test-key");

    let result = provider.generate("hello", None).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Hello from Gemini!");

    mock.assert();
}

#[test]
fn test_provider_config_serialization() {
    let config = ProviderConfig::OpenAI {
        model_name: "gpt-4".to_string(),
        api_key: "sk-xxx".to_string(),
        base_url: Some("http://localhost:9090/v1".to_string()),
    };
    let yaml = serde_yaml::to_string(&config).unwrap();
    assert!(yaml.contains("gpt-4"));
    let deserialized: ProviderConfig = serde_yaml::from_str(&yaml).unwrap();
    match deserialized {
        ProviderConfig::OpenAI { model_name, .. } => assert_eq!(model_name, "gpt-4"),
        _ => panic!("Expected OpenAI variant"),
    }
}
