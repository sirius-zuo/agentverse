use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::error::ModelError;
use crate::memory::{ContentBlock, MessageRole};
use crate::model::{GenerateRequest, GenerateResponse, UsageStats};

#[derive(Debug, Clone)]
pub struct OpenAICompatible {
    /// When true, sends `chat_template_kwargs: {"enable_thinking": false}`.
    /// Defaults to true — thinking is disabled unless `LLAMA_DISABLE_THINKING=0` is set.
    /// Keeping thinking off makes structured-output formats (ReAct) more reliable.
    disable_thinking: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_template_kwargs: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
struct ChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ChatToolCallOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ChatToolCallOut {
    id: String,
    #[serde(rename = "type")]
    type_field: &'static str, // "function"
    function: ChatFunctionCallOut,
}

#[derive(Debug, Clone, Serialize)]
struct ChatFunctionCallOut {
    name: String,
    arguments: String, // JSON-encoded, per OpenAI's wire format
}

#[derive(Debug, Clone, Serialize)]
struct ChatTool {
    #[serde(rename = "type")]
    type_field: String,
    function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
struct FunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
    strict: bool,
}

#[derive(Debug, Deserialize, Default)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: PromptTokensDetails,
}

#[derive(Debug, Deserialize, Default)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: ChatUsage,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallWire>,
}

#[derive(Debug, Deserialize)]
struct ToolCallWire {
    id: String,
    function: FunctionCallWire,
}

#[derive(Debug, Deserialize)]
struct FunctionCallWire {
    name: String,
    arguments: String,
}

fn read_disable_thinking() -> bool {
    std::env::var("LLAMA_DISABLE_THINKING")
        .map(|v| !(v == "0" || v.to_lowercase() == "false"))
        .unwrap_or(true)
}

/// Build a `ChatMessage` for a User/Assistant-role message: `Text` blocks
/// join into `content`, `ToolUse` blocks collect into `tool_calls`. A
/// `ToolResult` block here is a protocol violation (results only ever
/// belong on a Tool-role message) and is a hard error, not a silent drop.
fn build_text_message(role: &str, content: Vec<ContentBlock>) -> Result<ChatMessage, ModelError> {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => text_parts.push(text),
            ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(ChatToolCallOut {
                    id,
                    type_field: "function",
                    function: ChatFunctionCallOut {
                        name,
                        arguments: serde_json::to_string(&input)
                            .unwrap_or_else(|_| "{}".to_string()),
                    },
                });
            }
            ContentBlock::ToolResult { .. } => {
                return Err(ModelError::InvalidResponse(format!(
                    "{role}-role message must not contain a ToolResult block"
                )));
            }
        }
    }
    let content = if text_parts.is_empty() && tool_calls.is_empty() {
        Some(String::new())
    } else if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join("\n"))
    };
    Ok(ChatMessage {
        role: role.to_string(),
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        tool_call_id: None,
    })
}

impl OpenAICompatible {
    pub fn new() -> Self {
        Self {
            disable_thinking: read_disable_thinking(),
        }
    }
}

impl Default for OpenAICompatible {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelProvider for OpenAICompatible {
    fn name(&self) -> &str {
        "openai"
    }

    fn build_request(
        &self,
        model: &str,
        request: GenerateRequest,
    ) -> Result<serde_json::Value, ModelError> {
        let mut messages = Vec::new();

        // System → prepend as role:"system"
        if let Some(system) = request.system {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: Some(system),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Conversation messages — map roles. A Tool-role message expands
        // into one ChatMessage per ToolResult block (OpenAI requires a
        // separate role:"tool" message per result, each with its own
        // tool_call_id); User/Assistant-role messages stay one ChatMessage,
        // collecting any ToolUse blocks into a single tool_calls array.
        for m in request.messages {
            match m.role {
                MessageRole::System => continue, // already handled above
                MessageRole::Tool => {
                    for block in m.content {
                        match block {
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                            } => {
                                let content = if is_error {
                                    format!("Error: {content}")
                                } else {
                                    content
                                };
                                messages.push(ChatMessage {
                                    role: "tool".to_string(),
                                    content: Some(content),
                                    tool_calls: None,
                                    tool_call_id: Some(tool_use_id),
                                });
                            }
                            other => {
                                return Err(ModelError::InvalidResponse(format!(
                                    "Tool-role message must contain only ToolResult blocks, found {other:?}"
                                )));
                            }
                        }
                    }
                }
                MessageRole::User => messages.push(build_text_message("user", m.content)?),
                MessageRole::Assistant => {
                    messages.push(build_text_message("assistant", m.content)?)
                }
            }
        }

        let chat_tools = request.tools.map(|t| {
            t.into_iter()
                .map(|tool| ChatTool {
                    type_field: "function".to_string(),
                    function: FunctionDefinition {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                        strict: true,
                    },
                })
                .collect()
        });

        let chat_template_kwargs = if self.disable_thinking {
            Some(serde_json::json!({"enable_thinking": false}))
        } else {
            None
        };

        let response_format = request.response_format.map(|schema| {
            serde_json::json!({
                "type": "json_schema",
                "json_schema": { "name": "response", "schema": schema }
            })
        });

        let req = ChatRequest {
            model: model.to_string(),
            messages,
            tools: chat_tools,
            chat_template_kwargs,
            response_format,
        };

        serde_json::to_value(req).map_err(|e| ModelError::InvalidResponse(e.to_string()))
    }

    fn parse_response(&self, body: &str) -> Result<GenerateResponse, ModelError> {
        let chat_response: ChatResponse =
            serde_json::from_str(body).map_err(|e| ModelError::InvalidResponse(e.to_string()))?;

        let message = chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| ModelError::InvalidResponse("No content in response".to_string()))?;

        let mut content: Vec<crate::memory::ContentBlock> = Vec::new();
        if let Some(text) = message.content {
            // Some OpenAI-compatible servers (llama.cpp, vLLM, etc.) send
            // content: "" alongside tool_calls rather than content: null --
            // skip the empty block so it doesn't show up as a spurious
            // Text("") alongside the real ToolUse block(s).
            if !text.trim().is_empty() {
                content.push(crate::memory::ContentBlock::Text { text });
            }
        }
        for call in message.tool_calls {
            let input: Value = serde_json::from_str(&call.function.arguments).map_err(|e| {
                ModelError::InvalidResponse(format!("invalid tool_call arguments JSON: {e}"))
            })?;
            content.push(crate::memory::ContentBlock::ToolUse {
                id: call.id,
                name: call.function.name,
                input,
            });
        }

        if content.is_empty() {
            return Err(ModelError::InvalidResponse(
                "No content in response".to_string(),
            ));
        }

        Ok(GenerateResponse {
            content,
            usage: UsageStats {
                input_tokens: chat_response.usage.prompt_tokens,
                output_tokens: chat_response.usage.completion_tokens,
                cache_write_tokens: 0,
                cache_read_tokens: chat_response.usage.prompt_tokens_details.cached_tokens,
            },
        })
    }

    fn request_headers(&self, api_key: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let auth_value = format!("Bearer {}", api_key);
        if let Ok(val) = HeaderValue::from_str(&auth_value) {
            headers.insert("Authorization", val);
        }
        headers
    }

    fn endpoint_path(&self, _model: &str) -> String {
        "/chat/completions".to_string()
    }
}
