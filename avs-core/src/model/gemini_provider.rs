use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::config::ProviderConfig;
use crate::error::ModelError;
use crate::model::ToolDefinition;

#[derive(Debug, Clone)]
pub struct GeminiProvider {
    client: Client,
    api_base: String,
    model_name: String,
    api_key: String,
}

#[derive(Debug, Clone, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiToolConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum GeminiPart {
    Text { text: String },
}

#[derive(Debug, Clone, Serialize)]
struct GeminiToolConfig {
    functions: Vec<GeminiFunction>,
}

#[derive(Debug, Clone, Serialize)]
struct GeminiFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}

impl GeminiProvider {
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
            ProviderConfig::Gemini {
                model_name,
                api_key,
            } => Ok(Self {
                client: Client::new(),
                api_base: "https://generativelanguage.googleapis.com".to_string(),
                model_name,
                api_key,
            }),
            _ => Err(ModelError::ApiError(
                "ProviderConfig is not Gemini".to_string(),
            )),
        }
    }

    fn generate_content_url(&self) -> String {
        format!(
            "{}/v1beta/models/{}:generateContent",
            self.api_base, self.model_name
        )
    }
}

#[async_trait]
impl ModelProvider for GeminiProvider {
    fn name(&self) -> &str {
        &self.model_name
    }

    async fn generate(
        &self,
        prompt: &str,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String, ModelError> {
        let contents = vec![GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::Text {
                text: prompt.to_string(),
            }],
        }];

        let gemini_tools = tools.map(|t| {
            vec![GeminiToolConfig {
                functions: t
                    .into_iter()
                    .map(|tool| GeminiFunction {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    })
                    .collect(),
            }]
        });

        let request = GeminiRequest {
            contents,
            tools: gemini_tools,
        };

        let url = format!("{}?key={}", self.generate_content_url(), self.api_key);

        let response = self
            .client
            .post(&url)
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
                "Gemini rate limited: {}",
                body
            )));
        }

        if !status.is_success() {
            return Err(ModelError::ApiError(format!("HTTP {}: {}", status, body)));
        }

        let resp: GeminiResponse =
            serde_json::from_str(&body).map_err(|e| ModelError::InvalidResponse(e.to_string()))?;

        resp.candidates
            .into_iter()
            .next()
            .and_then(|c| {
                c.content.parts.into_iter().next().map(|p| match p {
                    GeminiPart::Text { text } => text,
                })
            })
            .ok_or_else(|| ModelError::InvalidResponse("No content in response".to_string()))
    }
}
