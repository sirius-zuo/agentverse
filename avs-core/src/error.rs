use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Model error: {0}")]
    Model(#[from] ModelError),
    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),
    #[error("Config error: {0}")]
    Config(#[from] ConfigError),
    #[error("Guardrail error: {0}")]
    Guardrail(#[from] GuardrailError),
    #[error("Memory error: {0}")]
    Memory(String),
    #[error("Interrupted: approval {0}")]
    Interrupted(Uuid),
}

#[derive(Error, Debug, Clone)]
pub enum ModelError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("Rate limited: {0}")]
    RateLimited(String),
    #[error("Circuit breaker open: {0}")]
    CircuitOpen(String),
    #[error("Unknown provider: {0}")]
    UnknownProvider(String),
    #[error("Invalid API key: {0}")]
    InvalidApiKey(String),
    #[error("Missing required setting {0:?} for provider {1:?}")]
    MissingSetting(String, String),
}

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Execution failed: {0}")]
    Execution(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid arguments: {0}")]
    InvalidArgs(String),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Invalid config: {0}")]
    Invalid(String),
    #[error("Missing field: {0}")]
    Missing(String),
    #[error("File error: {0}")]
    FileError(String),
}

#[derive(Error, Debug)]
pub enum GuardrailError {
    #[error("Prompt injection: {0}")]
    PromptInjection(String),
    #[error("Output filtered: {0}")]
    OutputFiltered(String),
}
