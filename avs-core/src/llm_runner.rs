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

        let model_name = config
            .provider
            .settings
            .get("model_name")
            .cloned()
            .unwrap_or_default();
        info!(model = %model_name, provider = %config.provider.name, "LlmRunner initialized");

        let registry = crate::model::ProviderRegistry::with_builtins();
        let cm = ConnectionManager::from_config(config.provider.clone(), &registry)?;
        Ok(Self {
            connection: Arc::new(cm),
        })
    }

    pub async fn invoke(&self, messages: Vec<Message>) -> Result<GenerateResponse, AgentError> {
        self.invoke_inner(messages, None).await
    }

    pub async fn invoke_structured(
        &self,
        messages: Vec<Message>,
        schema: serde_json::Value,
    ) -> Result<GenerateResponse, AgentError> {
        self.invoke_inner(messages, Some(schema)).await
    }

    async fn invoke_inner(
        &self,
        messages: Vec<Message>,
        response_format: Option<serde_json::Value>,
    ) -> Result<GenerateResponse, AgentError> {
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
                response_format,
            })
            .await
            .map_err(AgentError::Model)
    }
}
