use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::config::ProviderConfig;
use crate::error::ModelError;
use crate::model::ToolDefinition;

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    client: Client,
    api_base: String,
    model_name: String,
    api_key: String,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    max_tokens: usize,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

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
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.model_name
    }

    async fn generate(
        &self,
        prompt: &str,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String, ModelError> {
        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }];

        let anthropic_tools = tools.map(|t| {
            t.into_iter()
                .map(|tool| AnthropicTool {
                    name: tool.name,
                    description: tool.description,
                    input_schema: tool.parameters,
                })
                .collect()
        });

        let request = AnthropicRequest {
            model: self.model_name.clone(),
            messages,
            tools: anthropic_tools,
            max_tokens: 4096,
        };

        let response = self
            .client
            .post(format!("{}/v1/messages", self.api_base))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
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

        // Return first text content
        resp.content
            .into_iter()
            .find(|c| c.content_type == "text")
            .and_then(|c| c.text)
            .ok_or_else(|| ModelError::InvalidResponse("No text content in response".to_string()))
    }
}
