use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::config::ProviderConfig;
use crate::error::ModelError;
use crate::memory::MessageRole;
use crate::model::{GenerateRequest, GenerateResponse, ToolDefinition, UsageStats};

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    client: Client,
    api_base: String,
    model_name: String,
    api_key: String,
}

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
            AnthropicProvider::map_role(m.role).map(|role| AnthropicMessage {
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

// ── Constructor ───────────────────────────────────────────────────────────────

impl AnthropicProvider {
    pub fn new(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self {
            client: Client::new(),
            api_base: api_base.to_string(),
            model_name: model_name.to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub fn from_config(config: ProviderConfig) -> Result<Self, ModelError> {
        match config {
            ProviderConfig::Anthropic {
                model_name,
                api_key,
            } => Ok(Self {
                client: Client::new(),
                api_base: "https://api.anthropic.com".to_string(),
                model_name,
                api_key,
            }),
            _ => Err(ModelError::ApiError(
                "ProviderConfig is not Anthropic".to_string(),
            )),
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
}

// ── ModelProvider impl ────────────────────────────────────────────────────────

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.model_name
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        let wire_request = build_wire_request(&self.model_name, request);

        let response = self
            .client
            .post(format!("{}/v1/messages", self.api_base))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "prompt-caching-2024-07-31")
            .header("Content-Type", "application/json")
            .json(&wire_request)
            .send()
            .await
            .map_err(|e| ModelError::ApiError(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ModelError::ApiError(e.to_string()))?;

        if status == 429 {
            return Err(ModelError::RateLimited(format!(
                "Anthropic rate limited: {}",
                body
            )));
        }
        if !status.is_success() {
            return Err(ModelError::ApiError(format!("HTTP {}: {}", status, body)));
        }

        let resp: AnthropicResponse =
            serde_json::from_str(&body).map_err(|e| ModelError::InvalidResponse(e.to_string()))?;

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
            },
        );
        assert_eq!(
            wire.messages.len(),
            1,
            "System-role message must be filtered"
        );
        assert_eq!(wire.messages[0].content[0].text, "hi");
    }
}
