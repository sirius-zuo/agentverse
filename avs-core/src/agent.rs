use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::AgentError;
use crate::memory::{Memory, Message, ShortTermMemory};
use crate::prompt::PromptRegistry;
use crate::tracing::{DefaultTracer, Tracer};

#[allow(dead_code)]
pub struct Agent {
    config: Config,
    memory: Arc<RwLock<dyn Memory>>,
    prompt_registry: PromptRegistry,
    tracer: Box<dyn Tracer>,
}

impl Agent {
    pub fn builder() -> crate::builder::AgentBuilder {
        crate::builder::AgentBuilder::new()
    }

    pub fn from_config(config: Config) -> Result<Self, AgentError> {
        config.validate()?;

        Ok(Self {
            config,
            memory: Arc::new(RwLock::new(ShortTermMemory::new(100))),
            prompt_registry: PromptRegistry::new(),
            tracer: Box::new(DefaultTracer::default()),
        })
    }

    pub async fn invoke(&self, user_id: &str, input: &str) -> Result<String, AgentError> {
        let mut memory = self.memory.write().await;
        memory.append(Message {
            role: crate::memory::MessageRole::User,
            content: input.to_string(),
        });
        drop(memory);

        // TODO: Strategy loop will be implemented in avs-react
        // For now, return a placeholder
        let _ = user_id;
        Ok(format!("Processed: {}", input))
    }
}
