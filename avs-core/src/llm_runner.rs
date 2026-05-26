use std::sync::Arc;
use tracing::info;

use crate::config::Config;
use crate::error::AgentError;
use crate::memory::{Message, MessageRole};
use crate::model::{ConnectionManager, GenerateRequest, GenerateResponse};

pub struct LlmRunner {
    connection: Arc<ConnectionManager>,
}

impl LlmRunner {
    pub fn new(connection: Arc<ConnectionManager>) -> Self {
        Self { connection }
    }

    pub fn from_config(config: Config) -> Result<Self, AgentError> {
        config.validate()?;

        let (model_name, provider_name) = match &config.provider {
            crate::config::ProviderConfig::OpenAI { model_name, .. } => {
                (model_name.as_str(), "openai")
            }
            crate::config::ProviderConfig::Anthropic { model_name, .. } => {
                (model_name.as_str(), "anthropic")
            }
            crate::config::ProviderConfig::Gemini { model_name, .. } => {
                (model_name.as_str(), "gemini")
            }
        };
        info!(model = %model_name, provider = %provider_name, "LlmRunner initialized");

        let cm = ConnectionManager::from_config(config.provider.clone())?;
        Ok(Self {
            connection: Arc::new(cm),
        })
    }

    pub async fn invoke(&self, messages: Vec<Message>) -> Result<GenerateResponse, AgentError> {
        let mut system_parts: Vec<String> = Vec::new();
        let mut conv: Vec<Message> = Vec::new();

        for msg in messages {
            if matches!(msg.role, MessageRole::System) {
                system_parts.push(msg.content);
            } else {
                conv.push(msg);
            }
        }

        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };

        self.connection
            .generate(GenerateRequest {
                system,
                messages: conv,
                tools: None,
            })
            .await
            .map_err(AgentError::Model)
    }
}
