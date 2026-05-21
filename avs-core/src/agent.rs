use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::AgentError;
use crate::memory::{Memory, Message, ShortTermMemory};
use crate::model::{ModelProvider, ProviderWrapper};
use crate::prompt::{PromptConfig, PromptRegistry};
use crate::tracing::{DefaultTracer, Tracer};

#[allow(dead_code)]
pub struct Agent {
    config: Config,
    provider: Option<ProviderWrapper>,
    memory: Arc<RwLock<dyn Memory>>,
    prompt_registry: PromptRegistry,
    tracer: Box<dyn Tracer>,
}

impl Agent {
    pub fn builder() -> crate::builder::AgentBuilder {
        crate::builder::AgentBuilder::new()
    }

    /// Create from config with explicit prompt configuration.
    pub fn from_config_with_prompts(
        config: Config,
        prompt_config: &PromptConfig,
    ) -> Result<Self, AgentError> {
        config.validate()?;

        let prompt_registry = PromptRegistry::from_config(prompt_config)?;

        let provider = Some(ProviderWrapper::from_config(config.provider.clone())?);

        Ok(Self {
            config,
            provider,
            memory: Arc::new(RwLock::new(ShortTermMemory::new(100))),
            prompt_registry,
            tracer: Box::new(DefaultTracer::default()),
        })
    }

    pub fn from_config(config: Config) -> Result<Self, AgentError> {
        let prompt_config = PromptConfig {
            system_prompt: config.system_prompt.clone(),
            prompts_dir: config.prompts_dir.clone(),
            templates: std::collections::HashMap::new(),
            examples: std::collections::HashMap::new(),
        };
        Self::from_config_with_prompts(config, &prompt_config)
    }

    /// Get a reference to the prompt registry for strategy composition.
    pub fn prompt_registry(&self) -> &PromptRegistry {
        &self.prompt_registry
    }

    pub async fn invoke(&self, _user_id: &str, input: &str) -> Result<String, AgentError> {
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| AgentError::Model(crate::error::ModelError::ApiError("no provider configured".to_string())))?;

        let mut memory = self.memory.write().await;
        memory.append(Message {
            role: crate::memory::MessageRole::User,
            content: input.to_string(),
        });
        let messages = memory
            .last_n(100)
            .await
            .map_err(|e| AgentError::Memory(e.to_string()))?;
        drop(memory);

        let mut context = std::collections::HashMap::new();
        context.insert(
            "conversation".to_string(),
            serde_json::Value::String(input.to_string()),
        );
        let system = self
            .prompt_registry
            .render("system", context)
            .ok()
            .filter(|s| !s.is_empty());

        let response = provider
            .generate(crate::model::GenerateRequest {
                system,
                messages,
                tools: None,
            })
            .await?;

        let mut memory = self.memory.write().await;
        memory.append(Message {
            role: crate::memory::MessageRole::Assistant,
            content: response.content.clone(),
        });

        Ok(response.content)
    }
}
