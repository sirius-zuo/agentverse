use serde::{Deserialize, Serialize};

use crate::error::{AgentError, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model_api_key: String,
    pub model_name: String,
    pub max_messages: usize,
    #[serde(default)]
    pub tools: Vec<String>,
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, AgentError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AgentError::Config(ConfigError::Invalid(e.to_string())))?;
        let config: Config = serde_yaml::from_str(&content)
            .map_err(|e| AgentError::Config(ConfigError::Invalid(e.to_string())))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), AgentError> {
        if self.model_api_key.is_empty() {
            return Err(AgentError::Config(ConfigError::Missing(
                "model_api_key is required".to_string(),
            )));
        }
        if self.model_name.is_empty() {
            return Err(AgentError::Config(ConfigError::Missing(
                "model_name is required".to_string(),
            )));
        }
        Ok(())
    }
}
