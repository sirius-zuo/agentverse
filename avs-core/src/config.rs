use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{AgentError, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub settings: HashMap<String, String>,
}

impl ProviderConfig {
    pub fn anthropic(model_name: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            name: "anthropic".to_string(),
            settings: HashMap::from([
                ("model_name".to_string(), model_name.into()),
                ("api_key".to_string(), api_key.into()),
            ]),
        }
    }

    pub fn openai(
        model_name: impl Into<String>,
        api_key: impl Into<String>,
        base_url: Option<String>,
    ) -> Self {
        let mut settings = HashMap::from([
            ("model_name".to_string(), model_name.into()),
            ("api_key".to_string(), api_key.into()),
        ]);
        if let Some(base_url) = base_url {
            settings.insert("base_url".to_string(), base_url);
        }
        Self {
            name: "openai".to_string(),
            settings,
        }
    }

    pub fn gemini(model_name: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            name: "gemini".to_string(),
            settings: HashMap::from([
                ("model_name".to_string(), model_name.into()),
                ("api_key".to_string(), api_key.into()),
            ]),
        }
    }

    pub fn custom(name: impl Into<String>, settings: HashMap<String, String>) -> Self {
        Self {
            name: name.into(),
            settings,
        }
    }
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

    /// Provider-specific settings (e.g. "model_name is required") are validated
    /// later, at `ConnectionManager::from_config` time, by the registered
    /// provider's own factory — `Config::validate` has no `ProviderRegistry`
    /// and so cannot know what settings an arbitrary provider name requires.
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.provider.name.is_empty() {
            return Err(AgentError::Config(ConfigError::Missing(
                "provider.name is required".to_string(),
            )));
        }
        Ok(())
    }
}
