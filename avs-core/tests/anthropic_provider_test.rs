use agentverse::memory::{Message, MessageRole};
use agentverse::{AnthropicProvider, GenerateRequest, ModelProvider};
use serde_json::json;

// ── AnthropicProvider tests ───────────────────────────────────────────────────

#[test]
fn test_anthropic_provider_name() {
    let provider = AnthropicProvider::new();
    assert_eq!(provider.name(), "anthropic");
}

#[test]
fn test_anthropic_build_request_system_prompt() {
    let provider = AnthropicProvider::new();
    let req = GenerateRequest {
        system: Some("Be helpful.".to_string()),
        messages: vec![Message::text(MessageRole::User, "hello")],
        tools: None,
        ..Default::default()
    };
    let body = provider
        .build_request("claude-3-5-sonnet-20241022", req)
        .unwrap();
    let system = body["system"].as_array().unwrap();
    assert_eq!(system[0]["text"], "Be helpful.");
    assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn test_anthropic_request_headers_contains_required_headers() {
    let provider = AnthropicProvider::new();
    let headers = provider.request_headers("my-api-key");
    assert!(headers.contains_key("x-api-key"));
    assert!(headers.contains_key("anthropic-version"));
    assert!(headers.contains_key("anthropic-beta"));
    let key_val = headers["x-api-key"].to_str().unwrap();
    assert_eq!(key_val, "my-api-key");
}

#[test]
fn test_anthropic_endpoint_path() {
    let provider = AnthropicProvider::new();
    assert_eq!(provider.endpoint_path("any"), "/v1/messages");
}

#[test]
fn test_anthropic_parse_response_usage_maps_cache_tokens() {
    let provider = AnthropicProvider::new();
    let body = json!({
        "content": [{"type": "text", "text": "done"}],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 20,
            "cache_creation_input_tokens": 80,
            "cache_read_input_tokens": 60
        }
    })
    .to_string();
    let result = provider.parse_response(&body).unwrap();
    assert_eq!(result.usage.input_tokens, 100);
    assert_eq!(result.usage.output_tokens, 20);
    assert_eq!(result.usage.cache_write_tokens, 80);
    assert_eq!(result.usage.cache_read_tokens, 60);
}

#[test]
fn test_anthropic_parse_response_malformed_tool_use_with_no_text_is_error() {
    // A tool_use block with no "input" field is malformed and must error
    // loudly, not be silently dropped.
    let provider = AnthropicProvider::new();
    let body = json!({
        "content": [{"type": "tool_use", "id": "x", "name": "fn"}],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    })
    .to_string();
    let err = provider.parse_response(&body).unwrap_err();
    assert!(err.to_string().contains("missing id, name, or input"));
}

#[test]
fn test_anthropic_parse_response_extracts_tool_use() {
    let provider = AnthropicProvider::new();
    let body = json!({
        "content": [{"type": "tool_use", "id": "call_1", "name": "calculate", "input": {"a": 1}}],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    })
    .to_string();
    let result = provider.parse_response(&body).unwrap();
    assert_eq!(result.content.len(), 1);
}
