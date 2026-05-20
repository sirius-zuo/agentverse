use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectorError {
    #[error("Connection failed: {0}")]
    Connection(String),
    #[error("Auth failed: {0}")]
    Auth(String),
    #[error("Platform error: {0}")]
    Platform(String),
    #[error("EOF")]
    Eof,
}

#[derive(Error, Debug)]
pub enum InvokerError {
    #[error("Agent error: {0}")]
    Agent(String),
}

#[derive(Error, Debug)]
pub enum IntegrationError {
    #[error("Input error: {0}")]
    Input(ConnectorError),
    #[error("Invoker error: {0}")]
    Invoker(InvokerError),
    #[error("Output error on {connector}: {source}")]
    Output {
        connector: String,
        source: ConnectorError,
    },
    #[error("Config error: {0}")]
    Config(String),
}
