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
        // Caching render order: tools → system → messages
        // Cache breakpoint on the LAST item in each stable section.

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

        // 3. Messages — filter System role, preserve user/assistant/tool roles
        //    cache_control on the penultimate message (last stable turn before current user query)
        let mut messages: Vec<AnthropicMessage> = request
            .messages
            .into_iter()
            .filter_map(|m| {
                Self::map_role(m.role).map(|role| AnthropicMessage {
                    role,
                    content: vec![AnthropicContentBlock::text(m.content)],
                })
            })
            .collect();

        // Mark penultimate message's last block as cached (if at least 2 messages exist)
        if messages.len() >= 2 {
            let penultimate = messages.len() - 2;
            if let Some(block) = messages[penultimate].content.last_mut() {
                block.cache_control = Some(CacheControl {
                    cache_type: "ephemeral",
                });
            }
        }

        let wire_request = AnthropicRequest {
            model: self.model_name.clone(),
            system,
            messages,
            tools,
            max_tokens: 4096,
        };

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
