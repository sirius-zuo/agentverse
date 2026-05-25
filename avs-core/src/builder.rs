use crate::config::{Config, ProviderConfig};
use crate::error::AgentError;

pub struct LlmRunnerBuilder {
    config: Option<Config>,
}

impl LlmRunnerBuilder {
    pub fn new() -> Self {
        Self { config: None }
    }

    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    pub fn build(self) -> Result<crate::llm_runner::LlmRunner, AgentError> {
        let config = self.config.unwrap_or_else(|| Config {
            provider: ProviderConfig::OpenAI {
                model_name: String::new(),
                api_key: String::new(),
                base_url: None,
            },
            max_messages: 100,
            tools: Vec::new(),
            prompts_dir: None,
            system_prompt: None,
        });
        crate::llm_runner::LlmRunner::from_config(config)
    }
}

impl Default for LlmRunnerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
