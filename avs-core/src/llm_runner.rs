use std::sync::Arc;
use tracing::info;

use crate::config::Config;
use crate::error::AgentError;
use crate::memory::Message;
use crate::model::{ConnectionManager, GenerateRequest, GenerateResponse};
use crate::prompt::{PromptConfig, PromptRegistry};

pub struct LlmRunner {
    connection_manager: Arc<ConnectionManager>,
    prompt_registry: PromptRegistry,
}

impl LlmRunner {
    pub fn from_config(config: Config) -> Result<Self, AgentError> {
        let prompt_config = PromptConfig {
            system_prompt: config.system_prompt.clone(),
            prompts_dir: config.prompts_dir.clone(),
            templates: std::collections::HashMap::new(),
            examples: std::collections::HashMap::new(),
        };
        Self::from_config_with_prompts(config, &prompt_config)
    }

    pub fn from_config_with_prompts(config: Config, prompt_config: &PromptConfig) -> Result<Self, AgentError> {
        config.validate()?;

        let (model_name, provider_name) = match &config.provider {
            crate::config::ProviderConfig::OpenAI { model_name, .. } => (model_name.as_str(), "openai"),
            crate::config::ProviderConfig::Anthropic { model_name, .. } => (model_name.as_str(), "anthropic"),
            crate::config::ProviderConfig::Gemini { model_name, .. } => (model_name.as_str(), "gemini"),
        };
        info!(model = %model_name, provider = %provider_name, "LlmRunner initialized");

        let cm = ConnectionManager::from_config(config.provider.clone())?;
        let prompt_registry = PromptRegistry::from_config(prompt_config)?;

        Ok(Self {
            connection_manager: Arc::new(cm),
            prompt_registry,
        })
    }

    pub fn prompt_registry(&self) -> &PromptRegistry {
        &self.prompt_registry
    }

    /// Stateless LLM invocation. Caller supplies the full message history.
    pub async fn invoke(&self, messages: Vec<Message>) -> Result<GenerateResponse, AgentError> {
        let system = self.prompt_registry
            .render("system", std::collections::HashMap::new())
            .ok()
            .filter(|s| !s.is_empty());

        self.connection_manager.generate(GenerateRequest {
            system,
            messages,
            tools: None,
        }).await.map_err(AgentError::Model)
    }
}
