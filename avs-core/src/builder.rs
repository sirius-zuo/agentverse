use crate::config::Config;
use crate::error::AgentError;
use crate::model::ModelProvider;

pub struct AgentBuilder {
    model: Option<Box<dyn ModelProvider>>,
    max_messages: usize,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model: None,
            max_messages: 100,
        }
    }

    pub fn model(mut self, model: impl ModelProvider + 'static) -> Self {
        self.model = Some(Box::new(model));
        self
    }

    pub fn max_messages(mut self, max: usize) -> Self {
        self.max_messages = max;
        self
    }

    pub fn build(self) -> Result<crate::agent::Agent, AgentError> {
        if self.model.is_none() {
            return Err(AgentError::Config(crate::error::ConfigError::Missing(
                "model is required".to_string(),
            )));
        }

        let config = Config {
            model_api_key: String::new(),
            model_name: String::new(),
            max_messages: self.max_messages,
            tools: Vec::new(),
            prompts_dir: None,
            system_prompt: None,
        };

        crate::agent::Agent::from_config(config)
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
