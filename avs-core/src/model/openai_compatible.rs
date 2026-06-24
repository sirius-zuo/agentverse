use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::error::ModelError;
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
    content: String,
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
}

fn read_disable_thinking() -> bool {
    std::env::var("LLAMA_DISABLE_THINKING")
        .map(|v| !(v == "0" || v.to_lowercase() == "false"))
        .unwrap_or(true)
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
        use crate::memory::MessageRole;

        let mut messages = Vec::new();

        // System → prepend as role:"system"
        if let Some(system) = request.system {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: system,
            });
        }

        // Conversation messages — map roles
        for m in request.messages {
            let role = match m.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
                MessageRole::System => continue, // already handled above
            };
            messages.push(ChatMessage {
                role: role.to_string(),
                content: m.content,
            });
        }

        let chat_tools = request.tools.map(|t| {
            t.into_iter()
                .map(|tool| ChatTool {
                    type_field: "function".to_string(),
                    function: FunctionDefinition {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
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

        let content = chat_response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| ModelError::InvalidResponse("No content in response".to_string()))?;

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
