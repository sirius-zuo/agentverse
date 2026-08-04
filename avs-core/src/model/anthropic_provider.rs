use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::error::ModelError;
use crate::memory::{ContentBlock, MessageRole};
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
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

impl AnthropicContentBlock {
    fn new_text(text: String) -> Self {
        Self::Text {
            text,
            cache_control: None,
        }
    }

    fn new_text_cached(text: String) -> Self {
        Self::Text {
            text,
            cache_control: Some(CacheControl {
                cache_type: "ephemeral",
            }),
        }
    }

    fn new_tool_use(id: String, name: String, input: Value) -> Self {
        Self::ToolUse {
            id,
            name,
            input,
            cache_control: None,
        }
    }

    fn new_tool_result(tool_use_id: String, content: String, is_error: bool) -> Self {
        Self::ToolResult {
            tool_use_id,
            content,
            is_error,
            cache_control: None,
        }
    }

    /// The text payload, if this is a `Text` block. Used only by tests that
    /// pin down the system-prompt / plain-text wiring.
    #[cfg(test)]
    fn text(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } => Some(text),
            _ => None,
        }
    }

    #[cfg(test)]
    fn cache_control(&self) -> Option<&CacheControl> {
        match self {
            Self::Text { cache_control, .. }
            | Self::ToolUse { cache_control, .. }
            | Self::ToolResult { cache_control, .. } => cache_control.as_ref(),
        }
    }

    fn set_cache_control(&mut self, cc: CacheControl) {
        match self {
            Self::Text { cache_control, .. }
            | Self::ToolUse { cache_control, .. }
            | Self::ToolResult { cache_control, .. } => *cache_control = Some(cc),
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
    strict: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicOutputFormat {
    #[serde(rename = "type")]
    type_field: &'static str, // "json_schema"
    schema: Value,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicOutputConfig {
    format: AnthropicOutputFormat,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<AnthropicOutputConfig>,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    #[serde(default)]
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<Value>,
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
                strict: true,
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
        .map(|s| vec![AnthropicContentBlock::new_text_cached(s)])
        .unwrap_or_default();

    // 3. Messages — filter System role; map every content block 1:1;
    //    cache breakpoint on the penultimate message's last block
    let mut messages: Vec<AnthropicMessage> = request
        .messages
        .into_iter()
        .filter_map(|m| {
            let role = map_role(m.role)?;
            let content = m
                .content
                .into_iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => AnthropicContentBlock::new_text(text),
                    ContentBlock::ToolUse { id, name, input } => {
                        AnthropicContentBlock::new_tool_use(id, name, input)
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => AnthropicContentBlock::new_tool_result(tool_use_id, content, is_error),
                })
                .collect();
            Some(AnthropicMessage { role, content })
        })
        .collect();

    if messages.len() >= 2 {
        let penultimate = messages.len() - 2;
        if let Some(block) = messages[penultimate].content.last_mut() {
            block.set_cache_control(CacheControl {
                cache_type: "ephemeral",
            });
        }
    }

    let output_config = request.response_format.map(|schema| AnthropicOutputConfig {
        format: AnthropicOutputFormat {
            type_field: "json_schema",
            schema,
        },
    });

    AnthropicRequest {
        model: model_name.to_string(),
        system,
        messages,
        tools,
        max_tokens: 4096,
        output_config,
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

        // A malformed tool_use block (missing id/name/input) is a hard error,
        // not silently dropped — even when other usable blocks are present.
        // Otherwise a model's real tool call could vanish with no signal,
        // leaving only unrelated text as the (wrong) final answer.
        let mut content: Vec<ContentBlock> = Vec::new();
        for c in resp.content {
            match c.content_type.as_str() {
                "text" => {
                    if let Some(text) = c.text {
                        if !text.trim().is_empty() {
                            content.push(ContentBlock::Text { text });
                        }
                    }
                }
                "tool_use" => match (c.id, c.name, c.input) {
                    (Some(id), Some(name), Some(input)) => {
                        content.push(ContentBlock::ToolUse { id, name, input });
                    }
                    _ => {
                        return Err(ModelError::InvalidResponse(
                            "tool_use content block missing id, name, or input".to_string(),
                        ));
                    }
                },
                _ => {}
            }
        }

        if content.is_empty() {
            return Err(ModelError::InvalidResponse(
                "No text or tool_use content in response".to_string(),
            ));
        }

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
#[path = "anthropic_provider_tests.rs"]
mod tests;
