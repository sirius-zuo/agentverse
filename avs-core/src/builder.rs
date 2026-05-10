use crate::config::Config;
use crate::error::AgentError;
use crate::prompt::PromptConfig;

pub struct AgentBuilder {
    config: Option<Config>,
    system_prompt: Option<String>,
    prompts_dir: Option<String>,
}

impl AgentBuilder {
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

    pub fn build(self) -> Result<crate::agent::Agent, AgentError> {
        let config = self.config.unwrap_or_else(|| Config {
            model_api_key: String::new(),
            model_name: String::new(),
            max_messages: 100,
            tools: Vec::new(),
            prompts_dir: self.prompts_dir.clone(),
            system_prompt: self.system_prompt.clone(),
        });

        if config.model_api_key.is_empty() {
            return Err(AgentError::Config(crate::error::ConfigError::Missing(
                "model_api_key is required".to_string(),
            )));
        }
        if config.model_name.is_empty() {
            return Err(AgentError::Config(crate::error::ConfigError::Missing(
                "model_name is required".to_string(),
            )));
        }

        let prompt_config = PromptConfig {
            system_prompt: self.system_prompt,
            prompts_dir: self.prompts_dir,
            templates: std::collections::HashMap::new(),
            examples: std::collections::HashMap::new(),
        };

        crate::agent::Agent::from_config_with_prompts(config, &prompt_config)
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
