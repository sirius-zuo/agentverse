use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::config::ProviderConfig;
use crate::error::ModelError;
use crate::model::{GenerateRequest, GenerateResponse, UsageStats};

#[derive(Debug, Clone)]
pub struct OpenAICompatible {
    client: Client,
    api_base: String,
    model_name: String,
    api_key: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
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

impl OpenAICompatible {
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
            ProviderConfig::OpenAI {
                model_name,
                api_key,
                base_url,
            } => Ok(Self {
                client: Client::new(),
                api_base: base_url.unwrap_or_else(|| "http://localhost:9090/v1".to_string()),
                model_name,
                api_key,
            }),
            _ => Err(ModelError::ApiError(
                "ProviderConfig is not OpenAI".to_string(),
            )),
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.api_base)
    }
}

#[async_trait]
impl ModelProvider for OpenAICompatible {
    fn name(&self) -> &str {
        &self.model_name
    }

    async fn generate(
        &self,
        request: GenerateRequest,
    ) -> Result<GenerateResponse, ModelError> {
        use crate::memory::MessageRole;

        let mut messages = Vec::new();

        // System → prepend as role:"system"
        if let Some(system) = request.system {
            messages.push(ChatMessage { role: "system".to_string(), content: system });
        }

        // Conversation messages — map roles
        for m in request.messages {
            let role = match m.role {
                MessageRole::User      => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool      => "tool",
                MessageRole::System    => continue, // already handled above
            };
            messages.push(ChatMessage { role: role.to_string(), content: m.content });
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

        let req = ChatRequest { model: self.model_name.clone(), messages, tools: chat_tools };

        let response = self
            .client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| ModelError::ApiError(e.to_string()))?;

        let status = response.status();
        let body = response.text().await.map_err(|e| ModelError::ApiError(e.to_string()))?;

        if !status.is_success() {
            return Err(ModelError::ApiError(format!("HTTP {}: {}", status, body)));
        }

        let chat_response: ChatResponse =
            serde_json::from_str(&body).map_err(|e| ModelError::InvalidResponse(e.to_string()))?;

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
}
