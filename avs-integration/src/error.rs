// avs-integration/src/error.rs — final state:
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
pub enum IntegrationError {
    #[error("Input error: {0}")]
    Input(ConnectorError),
    #[error("Output error on {connector}: {source}")]
    Output {
        connector: String,
        source: ConnectorError,
    },
    #[error("Config error: {0}")]
    Config(String),
}
