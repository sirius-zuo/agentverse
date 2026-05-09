// avs-integration/src/adapter.rs
use agentverse::AgentError;
use async_trait::async_trait;

/// Trait for integration adapters.
/// Each adapter connects an external platform (Slack, Webhook, etc.) to an Agent.
#[async_trait]
pub trait IntegrationAdapter: Send + Sync {
    /// The name of this adapter (e.g., "slack", "webhook").
    fn name(&self) -> &str;

    /// Start the adapter (listen for incoming messages).
    async fn start(&self) -> Result<(), IntegrationError>;

    /// Stop the adapter.
    async fn stop(&self);

    /// Get the health status.
    async fn health_check(&self) -> Result<(), IntegrationError>;
}

#[derive(thiserror::Error, Debug)]
pub enum IntegrationError {
    #[error("Connection failed: {0}")]
    Connection(String),
    #[error("Agent error: {0}")]
    Agent(AgentError),
    #[error("Configuration error: {0}")]
    Config(String),
}
