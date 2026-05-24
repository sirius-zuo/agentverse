use crate::config::{Config, ProviderConfig};
use crate::error::AgentError;
use crate::prompt::PromptConfig;

pub struct LlmRunnerBuilder {
    config: Option<Config>,
    system_prompt: Option<String>,
    prompts_dir: Option<String>,
}

impl LlmRunnerBuilder {
    pub fn new() -> Self {
        Self {
            config: None,
            system_prompt: None,
            prompts_dir: None,
        }
    }

    /// Set the full config.
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Set a system prompt override.
    pub fn system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = Some(prompt.to_string());
        self
    }

    /// Set a prompts directory for .j2/.toml file loading.
    pub fn prompt_dir(mut self, dir: &str) -> Self {
        self.prompts_dir = Some(dir.to_string());
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
            prompts_dir: self.prompts_dir.clone(),
            system_prompt: self.system_prompt.clone(),
        });

        let prompt_config = PromptConfig {
            system_prompt: self.system_prompt,
            prompts_dir: self.prompts_dir,
            templates: std::collections::HashMap::new(),
            examples: std::collections::HashMap::new(),
        };

        crate::llm_runner::LlmRunner::from_config_with_prompts(config, &prompt_config)
    }
}

impl Default for LlmRunnerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
