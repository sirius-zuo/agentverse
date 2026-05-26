use serde::{Deserialize, Serialize};

use crate::error::{AgentError, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConfig {
    OpenAI {
        model_name: String,
        api_key: String,
        #[serde(default)]
        base_url: Option<String>,
    },
    Anthropic {
        model_name: String,
        api_key: String,
    },
    Gemini {
        model_name: String,
        api_key: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: ProviderConfig,
    pub max_messages: usize,
    #[serde(default)]
    pub tools: Vec<String>,
    /// Optional prompts directory for .j2/.toml file loading.
    #[serde(default)]
    pub prompts_dir: Option<String>,
    /// Optional system prompt override.
    #[serde(default)]
    pub system_prompt: Option<String>,
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
        match &self.provider {
            ProviderConfig::OpenAI {
                model_name,
                api_key,
                base_url,
            } => {
                if model_name.is_empty() {
                    return Err(AgentError::Config(ConfigError::Missing(
                        "provider.model_name is required".to_string(),
                    )));
                }
                // api_key is optional when a custom base_url is set (local/self-hosted endpoints)
                if api_key.is_empty() && base_url.is_none() {
                    return Err(AgentError::Config(ConfigError::Missing(
                        "provider.api_key is required".to_string(),
                    )));
                }
            }
            ProviderConfig::Anthropic {
                model_name,
                api_key,
            }
            | ProviderConfig::Gemini {
                model_name,
                api_key,
            } => {
                if model_name.is_empty() {
                    return Err(AgentError::Config(ConfigError::Missing(
                        "provider.model_name is required".to_string(),
                    )));
                }
                if api_key.is_empty() {
                    return Err(AgentError::Config(ConfigError::Missing(
                        "provider.api_key is required".to_string(),
                    )));
                }
            }
        }
        Ok(())
    }
}
