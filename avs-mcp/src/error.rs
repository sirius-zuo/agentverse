#[derive(thiserror::Error, Debug)]
pub enum McpError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Initialization failed: {0}")]
    Initialization(String),
    #[error("Protocol error: {0}")]
    Protocol(String),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Tool call error: {0}")]
    ToolCall(String),
    #[error("Config error: {0}")]
    Config(String),
}
