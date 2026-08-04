use agentverse::memory::{Message, MessageRole};
use agentverse::{GenerateRequest, ModelProvider, OpenAICompatible, ToolDefinition};
use serde_json::json;

// ── OpenAICompatible provider tests ───────────────────────────────────────────

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
    assert_eq!(
        messages.len(),
        2,
        "each ToolResult must become its own message"
    );
    assert_eq!(messages[0]["role"], "tool");
    assert_eq!(messages[0]["tool_call_id"], "call_1");
    assert_eq!(messages[0]["content"], "3");
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[1]["tool_call_id"], "call_2");
    assert_eq!(messages[1]["content"], "Error: boom");
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
fn test_openai_build_request_tool_role_message_with_non_tool_result_block_is_error() {
    let provider = OpenAICompatible::new();
    let request = GenerateRequest {
        system: None,
        messages: vec![Message {
            role: MessageRole::Tool,
            content: vec![agentverse::ContentBlock::Text {
                text: "stray tool text".to_string(),
            }],
        }],
        tools: None,
        ..Default::default()
    };
    let err = provider.build_request("gpt", request).unwrap_err();
    assert!(err.to_string().contains("Tool-role message"));
}

#[test]
fn test_openai_build_request_prefixes_error_tool_result_content() {
    let provider = OpenAICompatible::new();
    let request = GenerateRequest {
        system: None,
        messages: vec![Message {
            role: MessageRole::Tool,
            content: vec![agentverse::ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: "boom".to_string(),
                is_error: true,
            }],
        }],
        tools: None,
        ..Default::default()
    };
    let body = provider.build_request("gpt", request).unwrap();
    assert_eq!(body["messages"][0]["content"], "Error: boom");
}

#[test]
fn test_openai_build_request_assistant_message_with_no_content_serializes_empty_string() {
    let provider = OpenAICompatible::new();
    let request = GenerateRequest {
        system: None,
        messages: vec![Message {
            role: MessageRole::Assistant,
            content: vec![],
        }],
        tools: None,
        ..Default::default()
    };
    let body = provider.build_request("gpt", request).unwrap();
    assert_eq!(body["messages"][0]["content"], "");
    assert!(body["messages"][0].get("tool_calls").is_none());
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
