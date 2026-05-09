use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::error::ModelError;
use crate::model::ToolDefinition;

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

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
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

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.api_base)
    }
}

#[async_trait]
impl ModelProvider for OpenAICompatible {
    async fn generate(
        &self,
        prompt: &str,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String, ModelError> {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }];

        let chat_tools = tools.map(|t| {
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

        let request = ChatRequest {
            model: self.model_name.clone(),
            messages,
            tools: chat_tools,
        };

        let response = self
            .client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ModelError::ApiError(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ModelError::ApiError(e.to_string()))?;

        if !status.is_success() {
            return Err(ModelError::ApiError(format!("HTTP {}: {}", status, body)));
        }

        let chat_response: ChatResponse =
            serde_json::from_str(&body).map_err(|e| ModelError::InvalidResponse(e.to_string()))?;

        chat_response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| ModelError::InvalidResponse("No content in response".to_string()))
    }
}
