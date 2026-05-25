// avs-server/src/config.rs
use agentverse::ProviderConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub agent: AgentConfig,
    pub guardrails: GuardrailsConfig,
    pub session: SessionConfig,
    pub aether_registry_url: Option<String>,
    pub agent_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(default = "default_session_store")]
    pub store: String,
    #[serde(default = "default_session_database_url")]
    pub database_url: String,
}

fn default_session_store() -> String {
    std::env::var("SESSION_STORE").unwrap_or_else(|_| "sqlite".to_string())
}

fn default_session_database_url() -> String {
    std::env::var("SESSION_DB_URL").unwrap_or_else(|_| "sqlite:sessions.db".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub provider: ProviderConfig,
    pub max_iterations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailsConfig {
    pub enabled: bool,
    pub max_requests_per_minute: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            eprintln!(
                "WARNING: MODEL_API_KEY is not set. The server will start but model calls will fail."
            );
        }
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            agent: AgentConfig {
                provider: ProviderConfig::OpenAI {
                    model_name: std::env::var("MODEL_NAME").unwrap_or_else(|_| "gpt-4".to_string()),
                    api_key,
                    base_url: Some(
                        std::env::var("MODEL_BASE_URL")
                            .unwrap_or_else(|_| "http://localhost:9090/v1".to_string()),
                    ),
                },
                max_iterations: 10,
            },
            guardrails: GuardrailsConfig {
                enabled: true,
                max_requests_per_minute: 60,
            },
            session: SessionConfig {
                store: default_session_store(),
                database_url: default_session_database_url(),
            },
            aether_registry_url: std::env::var("AETHER_REGISTRY_URL").ok(),
            agent_name: std::env::var("AGENT_NAME")
                .unwrap_or_else(|_| "agentverse-agent".to_string()),
        }
    }
}

impl ServerConfig {
    pub fn from_file(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        let config: ServerConfig =
            serde_yaml::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;
        Ok(config)
    }

    pub fn from_env() -> Self {
        Self::default()
    }
}
