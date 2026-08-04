use super::*;
use crate::memory::{Message, MessageRole};

fn user(content: &str) -> Message {
    Message::text(MessageRole::User, content)
}

fn assistant(content: &str) -> Message {
    Message::text(MessageRole::Assistant, content)
}

fn tool_def(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: format!("{} description", name),
        parameters: serde_json::json!({"type":"object","properties":{}}),
    }
}

#[test]
fn system_block_is_always_cached() {
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: Some("You are helpful.".to_string()),
            messages: vec![user("hi")],
            tools: None,
            ..Default::default()
        },
    );
    assert_eq!(wire.system.len(), 1);
    assert_eq!(wire.system[0].text().unwrap(), "You are helpful.");
    assert_eq!(
        wire.system[0].cache_control().unwrap().cache_type,
        "ephemeral"
    );
}

#[test]
fn no_system_produces_empty_system_vec() {
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![user("hi")],
            tools: None,
            ..Default::default()
        },
    );
    assert!(wire.system.is_empty());
}

#[test]
fn last_tool_is_cached_others_are_not() {
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![user("hi")],
            tools: Some(vec![tool_def("alpha"), tool_def("beta"), tool_def("gamma")]),
            ..Default::default()
        },
    );
    let tools = wire.tools.unwrap();
    assert_eq!(tools.len(), 3);
    assert_eq!(tools[0].name, "alpha");
    assert_eq!(tools[0].description, "alpha description");
    assert_eq!(
        tools[0].input_schema,
        serde_json::json!({"type":"object","properties":{}})
    );
    assert!(tools[0].cache_control.is_none(), "alpha must not be cached");
    assert!(tools[1].cache_control.is_none(), "beta must not be cached");
    assert_eq!(
        tools[2].cache_control.as_ref().unwrap().cache_type,
        "ephemeral",
        "gamma (last) must be cached"
    );
}

#[test]
fn single_message_has_no_message_cache_breakpoint() {
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![user("only message")],
            tools: None,
            ..Default::default()
        },
    );
    assert_eq!(wire.messages.len(), 1);
    assert!(
        wire.messages[0].content[0].cache_control().is_none(),
        "single message must not be cached"
    );
}

#[test]
fn penultimate_message_gets_cache_breakpoint() {
    // [user1, assistant1, user2(current)]
    // Penultimate = assistant1 (index 1)
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![user("q1"), assistant("a1"), user("q2")],
            tools: None,
            ..Default::default()
        },
    );
    let msgs = &wire.messages;
    assert_eq!(msgs.len(), 3);
    assert!(
        msgs[0].content[0].cache_control().is_none(),
        "user1 not cached"
    );
    assert_eq!(
        msgs[1].content[0].cache_control().unwrap().cache_type,
        "ephemeral",
        "assistant1 (penultimate) must be cached"
    );
    assert!(
        msgs[2].content[0].cache_control().is_none(),
        "user2 (current) must not be cached"
    );
}

#[test]
fn two_messages_caches_first() {
    // [user1, user2(current)] → penultimate = user1
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![user("first"), user("second")],
            tools: None,
            ..Default::default()
        },
    );
    assert_eq!(
        wire.messages[0].content[0]
            .cache_control()
            .unwrap()
            .cache_type,
        "ephemeral"
    );
    assert!(wire.messages[1].content[0].cache_control().is_none());
}

#[test]
fn system_role_messages_are_filtered_out() {
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![Message::text(MessageRole::System, "ignored"), user("hi")],
            tools: None,
            ..Default::default()
        },
    );
    assert_eq!(
        wire.messages.len(),
        1,
        "System-role message must be filtered"
    );
    assert_eq!(wire.messages[0].content[0].text().unwrap(), "hi");
}

#[test]
fn build_request_serializes_system_prompt() {
    let provider = AnthropicProvider::new();
    let req = GenerateRequest {
        system: Some("You are helpful.".to_string()),
        messages: vec![Message::text(MessageRole::User, "hi")],
        tools: None,
        ..Default::default()
    };
    let body = provider
        .build_request("claude-3-5-sonnet-20241022", req)
        .unwrap();
    let system = body["system"].as_array().unwrap();
    assert_eq!(system[0]["text"], "You are helpful.");
}

#[test]
fn request_headers_contains_api_key() {
    let provider = AnthropicProvider::new();
    let headers = provider.request_headers("test-key");
    assert!(headers.contains_key("x-api-key"));
    assert!(headers.contains_key("anthropic-version"));
}

#[test]
fn endpoint_path_is_messages() {
    let provider = AnthropicProvider::new();
    assert_eq!(provider.endpoint_path("any-model"), "/v1/messages");
}

#[test]
fn parse_response_extracts_text_content() {
    let provider = AnthropicProvider::new();
    let body = r#"{
        "content": [{"type": "text", "text": "Hello!"}],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    }"#;
    let resp = provider.parse_response(body).unwrap();
    assert_eq!(resp.as_text(), "Hello!");
    assert_eq!(resp.usage.input_tokens, 10);
}

#[test]
fn response_format_serialized_as_output_config() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "nodes": { "type": "array" } }
    });
    let wire = build_wire_request(
        "claude-sonnet-4-6",
        GenerateRequest {
            system: None,
            messages: vec![user("plan this")],
            tools: None,
            response_format: Some(schema.clone()),
        },
    );
    let config = wire.output_config.expect("output_config must be set");
    assert_eq!(config.format.type_field, "json_schema");
    assert_eq!(config.format.schema, schema);
}

#[test]
fn response_format_none_omits_output_config() {
    let provider = AnthropicProvider::new();
    let body = provider
        .build_request(
            "claude-sonnet-4-6",
            GenerateRequest {
                system: None,
                messages: vec![user("hi")],
                tools: None,
                response_format: None,
            },
        )
        .unwrap();
    assert!(body.get("output_config").is_none());
}

#[test]
fn tools_are_sent_with_strict_true() {
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![user("hi")],
            tools: Some(vec![tool_def("alpha")]),
            ..Default::default()
        },
    );
    let tools = wire.tools.unwrap();
    assert!(tools[0].strict);
}

#[test]
fn parse_response_extracts_tool_use_block() {
    let provider = AnthropicProvider::new();
    let body = r#"{
        "content": [{"type": "tool_use", "id": "call_1", "name": "milestone_scheduler", "input": {"start_date": "2026-01-01"}}],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    }"#;
    let resp = provider.parse_response(body).unwrap();
    assert_eq!(resp.content.len(), 1);
    match &resp.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "milestone_scheduler");
            assert_eq!(input["start_date"], "2026-01-01");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn parse_response_extracts_text_and_tool_use_together() {
    let provider = AnthropicProvider::new();
    let body = r#"{
        "content": [
            {"type": "text", "text": "Let me check the schedule."},
            {"type": "tool_use", "id": "call_1", "name": "milestone_scheduler", "input": {}}
        ],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    }"#;
    let resp = provider.parse_response(body).unwrap();
    assert_eq!(resp.content.len(), 2);
    assert!(matches!(resp.content[0], ContentBlock::Text { .. }));
    assert!(matches!(resp.content[1], ContentBlock::ToolUse { .. }));
}

#[test]
fn parse_response_extracts_multiple_tool_use_blocks() {
    let provider = AnthropicProvider::new();
    let body = r#"{
        "content": [
            {"type": "tool_use", "id": "call_1", "name": "milestone_scheduler", "input": {}},
            {"type": "tool_use", "id": "call_2", "name": "risk_adjusted_schedule", "input": {}}
        ],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    }"#;
    let resp = provider.parse_response(body).unwrap();
    assert_eq!(resp.content.len(), 2);
    let ids: Vec<&str> = resp
        .content
        .iter()
        .map(|c| match c {
            ContentBlock::ToolUse { id, .. } => id.as_str(),
            other => panic!("expected ToolUse, got {other:?}"),
        })
        .collect();
    assert_eq!(ids, ["call_1", "call_2"]);
}

#[test]
fn parse_response_malformed_tool_use_is_error() {
    let provider = AnthropicProvider::new();
    // A tool_use block missing "input" is malformed and must error
    // loudly, not be silently dropped -- this is what keeps Anthropic
    // consistent with the OpenAI-compatible provider's hard-error-on-
    // malformed-arguments behavior.
    let body = r#"{
        "content": [{"type": "tool_use", "id": "x", "name": "fn"}],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    }"#;
    let err = provider.parse_response(body).unwrap_err();
    assert!(err.to_string().contains("missing id, name, or input"));
}

#[test]
fn parse_response_malformed_tool_use_errors_even_with_text_present() {
    let provider = AnthropicProvider::new();
    // The malformed tool_use must not be silently dropped just because
    // a text block is also present -- that would surface as a wrong
    // (text-only) final answer instead of a diagnosable error.
    let body = r#"{
        "content": [
            {"type": "text", "text": "Let me look that up."},
            {"type": "tool_use", "id": "x", "name": "fn"}
        ],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    }"#;
    let err = provider.parse_response(body).unwrap_err();
    assert!(err.to_string().contains("missing id, name, or input"));
}

#[test]
fn parse_response_truly_empty_content_is_error() {
    let provider = AnthropicProvider::new();
    let body = r#"{
        "content": [],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    }"#;
    let err = provider.parse_response(body).unwrap_err();
    assert!(err.to_string().contains("No text or tool_use content"));
}

#[test]
fn parse_response_skips_empty_text_blocks() {
    let provider = AnthropicProvider::new();
    let body = r#"{
        "content": [
            {"type": "text", "text": ""},
            {"type": "tool_use", "id": "call_1", "name": "fn", "input": {}}
        ],
        "usage": {"input_tokens": 10, "output_tokens": 5,
                  "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
    }"#;
    let resp = provider.parse_response(body).unwrap();
    assert_eq!(resp.content.len(), 1);
    assert!(matches!(resp.content[0], ContentBlock::ToolUse { .. }));
}

#[test]
fn strict_field_is_serialized_in_wire_json() {
    let provider = AnthropicProvider::new();
    let body = provider
        .build_request(
            "m",
            GenerateRequest {
                system: None,
                messages: vec![user("hi")],
                tools: Some(vec![tool_def("alpha")]),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(body["tools"][0]["strict"], true);
}

#[test]
fn build_wire_request_serializes_assistant_tool_use_block() {
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![Message {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "milestone_scheduler".to_string(),
                    input: serde_json::json!({"start_date": "2026-01-01"}),
                }],
            }],
            tools: None,
            ..Default::default()
        },
    );
    assert_eq!(wire.messages.len(), 1);
    assert_eq!(wire.messages[0].role, "assistant");
    assert_eq!(wire.messages[0].content.len(), 1);
    match &wire.messages[0].content[0] {
        AnthropicContentBlock::ToolUse {
            id, name, input, ..
        } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "milestone_scheduler");
            assert_eq!(input["start_date"], "2026-01-01");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn build_wire_request_serializes_tool_result_block() {
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![Message {
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: "42".to_string(),
                    is_error: false,
                }],
            }],
            tools: None,
            ..Default::default()
        },
    );
    assert_eq!(wire.messages.len(), 1);
    assert_eq!(wire.messages[0].role, "user");
    match &wire.messages[0].content[0] {
        AnthropicContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "call_1");
            assert_eq!(content, "42");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn build_wire_request_serializes_mixed_text_and_tool_use_in_one_message() {
    let wire = build_wire_request(
        "m",
        GenerateRequest {
            system: None,
            messages: vec![Message {
                role: MessageRole::Assistant,
                content: vec![
                    ContentBlock::Text {
                        text: "Let me check.".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        name: "fn".to_string(),
                        input: serde_json::json!({}),
                    },
                ],
            }],
            tools: None,
            ..Default::default()
        },
    );
    assert_eq!(wire.messages[0].content.len(), 2);
    assert!(matches!(
        wire.messages[0].content[0],
        AnthropicContentBlock::Text { .. }
    ));
    assert!(matches!(
        wire.messages[0].content[1],
        AnthropicContentBlock::ToolUse { .. }
    ));
}

#[test]
fn build_request_tool_use_block_has_correct_wire_json_shape() {
    let provider = AnthropicProvider::new();
    let body = provider
        .build_request(
            "m",
            GenerateRequest {
                system: None,
                messages: vec![Message {
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        name: "fn".to_string(),
                        input: serde_json::json!({"x": 1}),
                    }],
                }],
                tools: None,
                ..Default::default()
            },
        )
        .unwrap();
    let block = &body["messages"][0]["content"][0];
    assert_eq!(block["type"], "tool_use");
    assert_eq!(block["id"], "call_1");
    assert_eq!(block["name"], "fn");
    assert_eq!(block["input"]["x"], 1);
}
