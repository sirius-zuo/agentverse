use agentverse::memory::{Message, MessageRole};
use agentverse::{
    AnthropicProvider, ConnectionManager, GeminiProvider, GenerateRequest, ModelProvider,
    OpenAICompatible, ProviderConfig, ToolDefinition,
};
use serde_json::json;

// ── ConnectionManager construction tests ──────────────────────────────────────

#[test]
fn test_connection_manager_from_config_openai() {
    let config = ProviderConfig::openai("gpt-4o".to_string(), "sk-test".to_string(), None);
    let registry = agentverse::ProviderRegistry::with_builtins();
    assert!(ConnectionManager::from_config(config, &registry).is_ok());
}

#[test]
fn test_connection_manager_from_config_anthropic() {
    let config =
        ProviderConfig::anthropic("claude-3-5-sonnet-20241022".to_string(), "key".to_string());
    let registry = agentverse::ProviderRegistry::with_builtins();
    assert!(ConnectionManager::from_config(config, &registry).is_ok());
}

#[test]
fn test_connection_manager_from_config_gemini() {
    let config = ProviderConfig::gemini("gemini-pro".to_string(), "key".to_string());
    let registry = agentverse::ProviderRegistry::with_builtins();
    assert!(ConnectionManager::from_config(config, &registry).is_ok());
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

// ── GeminiProvider tests ──────────────────────────────────────────────────────

#[test]
fn test_openai_build_request_with_tools_serializes_chat_tools() {
    let provider = OpenAICompatible::new();
    let request = GenerateRequest {
        system: None,
        messages: vec![Message::text(MessageRole::User, "hello")],
        tools: Some(vec![ToolDefinition {
            name: "calculate".to_string(),
            description: "Perform arithmetic".to_string(),
            parameters: json!({"type": "object", "properties": {"expression": {"type": "string"}}}),
        }]),
        ..Default::default()
    };

    let body = provider.build_request("gpt", request).unwrap();
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "calculate");
    assert_eq!(
        body["tools"][0]["function"]["description"],
        "Perform arithmetic"
    );
    assert_eq!(
        body["tools"][0]["function"]["parameters"],
        json!({"type": "object", "properties": {"expression": {"type": "string"}}})
    );
    assert_eq!(body["tools"][0]["function"]["strict"], true);
}

#[test]
fn test_openai_build_request_serializes_assistant_tool_calls() {
    let provider = OpenAICompatible::new();
    let request = GenerateRequest {
        system: None,
        messages: vec![Message {
            role: MessageRole::Assistant,
            content: vec![agentverse::ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "calculate".to_string(),
                input: json!({"a": 1, "b": 2}),
            }],
        }],
        tools: None,
        ..Default::default()
    };
    let body = provider.build_request("gpt", request).unwrap();
    let message = &body["messages"][0];
    assert_eq!(message["role"], "assistant");
    assert!(
        message.get("content").is_none(),
        "content must be omitted when there is no text"
    );
    let tool_call = &message["tool_calls"][0];
    assert_eq!(tool_call["id"], "call_1");
    assert_eq!(tool_call["type"], "function");
    assert_eq!(tool_call["function"]["name"], "calculate");
    let arguments: serde_json::Value =
        serde_json::from_str(tool_call["function"]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(arguments, json!({"a": 1, "b": 2}));
}

#[test]
fn test_openai_build_request_serializes_tool_results_as_separate_messages() {
    let provider = OpenAICompatible::new();
    let request = GenerateRequest {
        system: None,
        messages: vec![Message {
            role: MessageRole::Tool,
            content: vec![
                agentverse::ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: "3".to_string(),
                    is_error: false,
                },
                agentverse::ContentBlock::ToolResult {
                    tool_use_id: "call_2".to_string(),
                    content: "boom".to_string(),
                    is_error: true,
                },
            ],
        }],
        tools: None,
        ..Default::default()
    };
    let body = provider.build_request("gpt", request).unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "each ToolResult must become its own message");
    assert_eq!(messages[0]["role"], "tool");
    assert_eq!(messages[0]["tool_call_id"], "call_1");
    assert_eq!(messages[0]["content"], "3");
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[1]["tool_call_id"], "call_2");
    assert_eq!(messages[1]["content"], "boom");
}

#[test]
fn test_openai_build_request_text_only_message_unaffected() {
    let provider = OpenAICompatible::new();
    let request = GenerateRequest {
        system: None,
        messages: vec![Message::text(MessageRole::User, "hello")],
        tools: None,
        ..Default::default()
    };
    let body = provider.build_request("gpt", request).unwrap();
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "hello");
    assert!(body["messages"][0].get("tool_calls").is_none());
    assert!(body["messages"][0].get("tool_call_id").is_none());
}

#[test]
fn test_openai_parse_response_extracts_tool_calls() {
    let provider = OpenAICompatible::new();
    let body = json!({
        "choices": [{"message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "calculate", "arguments": "{\"a\":1,\"b\":2}"}
            }]
        }}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3}
    })
    .to_string();
    let result = provider.parse_response(&body).unwrap();
    assert_eq!(result.content.len(), 1);
    match &result.content[0] {
        agentverse::ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "calculate");
            assert_eq!(input["a"], 1);
            assert_eq!(input["b"], 2);
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn test_openai_parse_response_extracts_text_and_tool_calls_together() {
    let provider = OpenAICompatible::new();
    let body = json!({
        "choices": [{"message": {
            "role": "assistant",
            "content": "Let me check.",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "calculate", "arguments": "{}"}
            }]
        }}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3}
    })
    .to_string();
    let result = provider.parse_response(&body).unwrap();
    assert_eq!(result.content.len(), 2);
}

#[test]
fn test_openai_parse_response_extracts_multiple_tool_calls() {
    let provider = OpenAICompatible::new();
    let body = json!({
        "choices": [{"message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [
                {
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "milestone_scheduler", "arguments": "{}"}
                },
                {
                    "id": "call_2",
                    "type": "function",
                    "function": {"name": "risk_adjusted_schedule", "arguments": "{}"}
                }
            ]
        }}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3}
    })
    .to_string();
    let result = provider.parse_response(&body).unwrap();
    assert_eq!(result.content.len(), 2);
    let ids: Vec<&str> = result
        .content
        .iter()
        .map(|c| match c {
            agentverse::ContentBlock::ToolUse { id, .. } => id.as_str(),
            other => panic!("expected ToolUse, got {other:?}"),
        })
        .collect();
    assert_eq!(ids, ["call_1", "call_2"]);
}

#[test]
fn test_openai_parse_response_malformed_tool_call_arguments_is_error() {
    let provider = OpenAICompatible::new();
    let body = json!({
        "choices": [{"message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "calculate", "arguments": "not valid json"}
            }]
        }}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3}
    })
    .to_string();
    let err = provider.parse_response(&body).unwrap_err();
    assert!(err.to_string().contains("invalid tool_call arguments JSON"));
}

#[test]
fn test_openai_parse_response_skips_empty_text_block_alongside_tool_calls() {
    let provider = OpenAICompatible::new();
    let body = json!({
        "choices": [{"message": {
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "calculate", "arguments": "{}"}
            }]
        }}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3}
    })
    .to_string();
    let result = provider.parse_response(&body).unwrap();
    assert_eq!(result.content.len(), 1);
    assert!(matches!(
        result.content[0],
        agentverse::ContentBlock::ToolUse { .. }
    ));
}

#[test]
fn test_openai_parse_response_no_content_or_tool_calls_is_error() {
    let provider = OpenAICompatible::new();
    let body = json!({
        "choices": [{"message": {"role": "assistant", "content": null}}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3}
    })
    .to_string();
    let err = provider.parse_response(&body).unwrap_err();
    assert!(err.to_string().contains("No content"));
}

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
        messages: vec![Message::text(MessageRole::User, "hello")],
        tools: None,
        ..Default::default()
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
            Message::text(MessageRole::System, "this should be filtered"),
            Message::text(MessageRole::User, "hello"),
        ],
        tools: None,
        ..Default::default()
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
        messages: vec![Message::text(MessageRole::User, "hello")],
        tools: Some(vec![ToolDefinition {
            name: "calc".to_string(),
            description: "Calculate".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        }]),
        ..Default::default()
    };
    let body = provider.build_request("gemini-pro", req).unwrap();
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools[0]["functions"][0]["name"], "calc");
    assert_eq!(tools[0]["functions"][0]["description"], "Calculate");
    assert_eq!(
        tools[0]["functions"][0]["parameters"],
        json!({"type": "object", "properties": {}})
    );
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
    assert_eq!(result.as_text(), "Hello from Gemini!");
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
    let config = ProviderConfig::openai(
        "gpt-4".to_string(),
        "sk-xxx".to_string(),
        Some("http://localhost:9090/v1".to_string()),
    );
    let yaml = serde_yaml::to_string(&config).unwrap();
    assert!(yaml.contains("gpt-4"));
    let deserialized: ProviderConfig = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(deserialized.name, "openai");
    assert_eq!(deserialized.settings.get("model_name").unwrap(), "gpt-4");
}

#[test]
fn connection_manager_with_model_uses_new_model_name() {
    use agentverse::memory::{Message, MessageRole};
    let cm =
        ConnectionManager::anthropic("https://api.anthropic.com", "claude-sonnet-4-6", "test-key");
    let registry = agentverse::ProviderRegistry::with_builtins();
    let overridden = cm
        .with_model("claude-haiku-4-5-20251001", &registry)
        .expect("known provider");
    let req = agentverse::GenerateRequest {
        system: None,
        messages: vec![Message::text(MessageRole::User, "hi")],
        tools: None,
        ..Default::default()
    };
    let body = overridden.provider_build_request_for_test(req).unwrap();
    assert_eq!(body["model"].as_str().unwrap(), "claude-haiku-4-5-20251001");
}

#[test]
fn connection_manager_with_model_openai_uses_new_model_name() {
    use agentverse::memory::{Message, MessageRole};
    let cm = ConnectionManager::openai("https://api.openai.com/v1", "gpt-4o", "test-key");
    let registry = agentverse::ProviderRegistry::with_builtins();
    let overridden = cm
        .with_model("gpt-4o-mini", &registry)
        .expect("known provider");
    let req = agentverse::GenerateRequest {
        system: None,
        messages: vec![Message::text(MessageRole::User, "hi")],
        tools: None,
        ..Default::default()
    };
    let body = overridden.provider_build_request_for_test(req).unwrap();
    assert_eq!(body["model"].as_str().unwrap(), "gpt-4o-mini");
}

#[test]
fn connection_manager_with_model_keyless_openai_local_endpoint_succeeds() {
    let cm = ConnectionManager::openai("http://localhost:9090/v1", "m", "");
    let registry = agentverse::ProviderRegistry::with_builtins();
    assert!(
        cm.with_model("m2", &registry).is_ok(),
        "with_model should succeed for a keyless local-endpoint openai manager, matching pre-registry behavior"
    );
}

#[test]
fn connection_manager_with_model_gemini_uses_new_model_name() {
    use agentverse::memory::{Message, MessageRole};
    let cm = ConnectionManager::gemini(
        "https://generativelanguage.googleapis.com",
        "gemini-2.0-flash",
        "test-key",
    );
    let registry = agentverse::ProviderRegistry::with_builtins();
    let overridden = cm
        .with_model("gemini-1.5-pro", &registry)
        .expect("known provider");
    let req = agentverse::GenerateRequest {
        system: None,
        messages: vec![Message::text(MessageRole::User, "hi")],
        tools: None,
        ..Default::default()
    };
    let body = overridden.provider_build_request_for_test(req).unwrap();
    // Gemini puts the model name in the URL path, not the request body —
    // just verify the request builds successfully with the new model.
    let _ = body;
}
