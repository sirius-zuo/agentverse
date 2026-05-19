use crate::error::AgentError;

#[async_trait::async_trait]
pub trait RunStrategy: Send {
    async fn process(&mut self, input: String) -> Result<String, AgentError>;
}
