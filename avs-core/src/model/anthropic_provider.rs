use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::error::ModelError;
use crate::memory::MessageRole;
use crate::model::{GenerateRequest, GenerateResponse, ToolDefinition, UsageStats};

#[derive(Debug, Clone, Default)]
pub struct AnthropicProvider;

// ── Wire types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    cache_type: &'static str, // "ephemeral"
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: &'static str, // "text"
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

impl AnthropicContentBlock {
    fn text(text: String) -> Self {
        Self {
            block_type: "text",
            text,
            cache_control: None,
        }
    }

    fn text_cached(text: String) -> Self {
        Self {
            block_type: "text",
            text,
            cache_control: Some(CacheControl {
                cache_type: "ephemeral",
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicMessage {
    role: &'static str,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicRequest {
    model: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    system: Vec<AnthropicContentBlock>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    max_tokens: usize,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    #[serde(default)]
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Build the Anthropic wire request from a `GenerateRequest`, applying cache
/// markers. Split out so tests can call it directly without an HTTP roundtrip.
fn build_wire_request(model_name: &str, request: GenerateRequest) -> AnthropicRequest {
    // 1. Tools — cache_control on the last entry
    let tools = request.tools.map(|defs| {
        let mut tools: Vec<AnthropicTool> = defs
            .into_iter()
            .map(|d: ToolDefinition| AnthropicTool {
                name: d.name,
                description: d.description,
                input_schema: d.parameters,
                cache_control: None,
            })
            .collect();
        if let Some(last) = tools.last_mut() {
            last.cache_control = Some(CacheControl {
                cache_type: "ephemeral",
            });
        }
        tools
    });

    // 2. System — single block, always cached
    let system: Vec<AnthropicContentBlock> = request
        .system
        .map(|s| vec![AnthropicContentBlock::text_cached(s)])
        .unwrap_or_default();

    // 3. Messages — filter System role; cache breakpoint on the penultimate
    let mut messages: Vec<AnthropicMessage> = request
        .messages
        .into_iter()
        .filter_map(|m| {
            map_role(m.role).map(|role| AnthropicMessage {
                role,
                content: vec![AnthropicContentBlock::text(m.content)],
            })
        })
        .collect();

    if messages.len() >= 2 {
        let penultimate = messages.len() - 2;
        if let Some(block) = messages[penultimate].content.last_mut() {
            block.cache_control = Some(CacheControl {
                cache_type: "ephemeral",
            });
        }
    }

    AnthropicRequest {
        model: model_name.to_string(),
        system,
        messages,
        tools,
        max_tokens: 4096,
    }
}

/// Map MessageRole to Anthropic role string. Returns None for System (filtered out).
fn map_role(role: MessageRole) -> Option<&'static str> {
    match role {
        MessageRole::User => Some("user"),
        MessageRole::Assistant => Some("assistant"),
        MessageRole::Tool => Some("user"), // tool results are user-turn content
        MessageRole::System => None,       // filtered: system goes in the system field
    }
}

// ── Constructor ───────────────────────────────────────────────────────────────

impl AnthropicProvider {
    pub fn new() -> Self {
        Self
    }
}

// ── ModelProvider impl ────────────────────────────────────────────────────────

impl ModelProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn build_request(
        &self,
        model: &str,
        request: GenerateRequest,
    ) -> Result<serde_json::Value, ModelError> {
        let wire = build_wire_request(model, request);
        serde_json::to_value(wire).map_err(|e| ModelError::InvalidResponse(e.to_string()))
    }

    fn parse_response(&self, body: &str) -> Result<GenerateResponse, ModelError> {
        let resp: AnthropicResponse =
            serde_json::from_str(body).map_err(|e| ModelError::InvalidResponse(e.to_string()))?;

        let content = resp
            .content
            .into_iter()
            .find(|c| c.content_type == "text")
            .and_then(|c| c.text)
            .ok_or_else(|| {
                ModelError::InvalidResponse("No text content in response".to_string())
            })?;

        Ok(GenerateResponse {
            content,
            usage: UsageStats {
                input_tokens: resp.usage.input_tokens,
                output_tokens: resp.usage.output_tokens,
                cache_write_tokens: resp.usage.cache_creation_input_tokens,
                cache_read_tokens: resp.usage.cache_read_input_tokens,
            },
        })
    }

    fn request_headers(&self, api_key: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(api_key).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert(
            "anthropic-beta",
            HeaderValue::from_static("prompt-caching-2024-07-31"),
        );
        headers
    }

    fn endpoint_path(&self, _model: &str) -> String {
        "/v1/messages".to_string()
    }
}

// ── Cache-marker unit tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Message, MessageRole};

    fn user(content: &str) -> Message {
        Message {
            role: MessageRole::User,
            content: content.to_string(),
        }
    }

    fn assistant(content: &str) -> Message {
        Message {
            role: MessageRole::Assistant,
            content: content.to_string(),
        }
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
        assert_eq!(wire.system[0].text, "You are helpful.");
        assert_eq!(
            wire.system[0].cache_control.as_ref().unwrap().cache_type,
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
            wire.messages[0].content[0].cache_control.is_none(),
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
            msgs[0].content[0].cache_control.is_none(),
            "user1 not cached"
        );
        assert_eq!(
            msgs[1].content[0]
                .cache_control
                .as_ref()
                .unwrap()
                .cache_type,
            "ephemeral",
            "assistant1 (penultimate) must be cached"
        );
        assert!(
            msgs[2].content[0].cache_control.is_none(),
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
                .cache_control
                .as_ref()
                .unwrap()
                .cache_type,
            "ephemeral"
        );
        assert!(wire.messages[1].content[0].cache_control.is_none());
    }

    #[test]
    fn system_role_messages_are_filtered_out() {
        let wire = build_wire_request(
            "m",
            GenerateRequest {
                system: None,
                messages: vec![
                    Message {
                        role: MessageRole::System,
                        content: "ignored".to_string(),
                    },
                    user("hi"),
                ],
                tools: None,
                ..Default::default()
            },
        );
        assert_eq!(
            wire.messages.len(),
            1,
            "System-role message must be filtered"
        );
        assert_eq!(wire.messages[0].content[0].text, "hi");
    }

    #[test]
    fn build_request_serializes_system_prompt() {
        let provider = AnthropicProvider::new();
        let req = GenerateRequest {
            system: Some("You are helpful.".to_string()),
            messages: vec![Message {
                role: MessageRole::User,
                content: "hi".into(),
            }],
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
        assert_eq!(resp.content, "Hello!");
        assert_eq!(resp.usage.input_tokens, 10);
    }
}
