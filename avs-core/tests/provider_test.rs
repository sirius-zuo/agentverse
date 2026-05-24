use agentverse::memory::{Message, MessageRole};
use agentverse::{
    AnthropicProvider, ConnectionManager, GeminiProvider, GenerateRequest, ModelProvider,
    ProviderConfig, ToolDefinition,
};
use serde_json::json;

// ── ConnectionManager construction tests ──────────────────────────────────────

#[test]
fn test_connection_manager_from_config_openai() {
    let config = ProviderConfig::OpenAI {
        model_name: "gpt-4o".to_string(),
        api_key: "sk-test".to_string(),
        base_url: None,
    };
    assert!(ConnectionManager::from_config(config).is_ok());
}

#[test]
fn test_connection_manager_from_config_anthropic() {
    let config = ProviderConfig::Anthropic {
        model_name: "claude-3-5-sonnet-20241022".to_string(),
        api_key: "key".to_string(),
    };
    assert!(ConnectionManager::from_config(config).is_ok());
}

#[test]
fn test_connection_manager_from_config_gemini() {
    let config = ProviderConfig::Gemini {
        model_name: "gemini-pro".to_string(),
        api_key: "key".to_string(),
    };
    assert!(ConnectionManager::from_config(config).is_ok());
}

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
        messages: vec![Message {
            role: MessageRole::User,
            content: "hello".to_string(),
        }],
        tools: None,
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
fn test_anthropic_parse_response_no_text_content_is_error() {
    let provider = AnthropicProvider::new();
    let body = json!({
        "content": [{"type": "tool_use", "id": "x", "name": "fn"}],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    })
    .to_string();
    let err = provider.parse_response(&body).unwrap_err();
    assert!(err.to_string().contains("No text content"));
}

// ── GeminiProvider tests ──────────────────────────────────────────────────────

#[test]
fn test_gemini_provider_name() {
    let provider = GeminiProvider::new();
    assert_eq!(provider.name(), "gemini");
}

#[test]
fn test_gemini_endpoint_path_includes_model() {
    let provider = GeminiProvider::new();
    assert_eq!(
        provider.endpoint_path("gemini-1.5-pro"),
        "/v1beta/models/gemini-1.5-pro:generateContent"
    );
}

#[test]
fn test_gemini_request_headers_is_empty() {
    let provider = GeminiProvider::new();
    let headers = provider.request_headers("my-api-key");
    // Gemini uses query param auth, not headers
    assert!(headers.is_empty());
}

#[test]
fn test_gemini_build_request_system_instruction() {
    let provider = GeminiProvider::new();
    let req = GenerateRequest {
        system: Some("System instruction".to_string()),
        messages: vec![Message {
            role: MessageRole::User,
            content: "hello".to_string(),
        }],
        tools: None,
    };
    let body = provider.build_request("gemini-pro", req).unwrap();
    assert_eq!(
        body["system_instruction"]["parts"][0]["text"],
        "System instruction"
    );
}

#[test]
fn test_gemini_build_request_system_role_messages_filtered() {
    let provider = GeminiProvider::new();
    let req = GenerateRequest {
        system: Some("System instruction".to_string()),
        messages: vec![
            Message {
                role: MessageRole::System,
                content: "this should be filtered".to_string(),
            },
            Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            },
        ],
        tools: None,
    };
    let body = provider.build_request("gemini-pro", req).unwrap();
    let contents = body["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["role"], "user");
}

#[test]
fn test_gemini_build_request_with_tools() {
    let provider = GeminiProvider::new();
    let req = GenerateRequest {
        system: None,
        messages: vec![Message {
            role: MessageRole::User,
            content: "hello".to_string(),
        }],
        tools: Some(vec![ToolDefinition {
            name: "calc".to_string(),
            description: "Calculate".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        }]),
    };
    let body = provider.build_request("gemini-pro", req).unwrap();
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools[0]["functions"][0]["name"], "calc");
}

#[test]
fn test_gemini_parse_response_ok() {
    let provider = GeminiProvider::new();
    let body = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "Hello from Gemini!"}]
            }
        }]
    })
    .to_string();
    let result = provider.parse_response(&body).unwrap();
    assert_eq!(result.content, "Hello from Gemini!");
}

#[test]
fn test_gemini_parse_response_empty_candidates_is_error() {
    let provider = GeminiProvider::new();
    let body = json!({"candidates": []}).to_string();
    let err = provider.parse_response(&body).unwrap_err();
    assert!(err.to_string().contains("No content"));
}

// ── ProviderConfig serialization ──────────────────────────────────────────────

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
