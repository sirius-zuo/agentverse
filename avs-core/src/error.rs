use thiserror::Error;

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
}

#[derive(Error, Debug)]
pub enum ModelError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Execution failed: {0}")]
    Execution(String),
    #[error("Not found: {0}")]
    NotFound(String),
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
