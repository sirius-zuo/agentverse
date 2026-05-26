use crate::error::AgentError;
use crate::memory::Message;

#[async_trait::async_trait]
pub trait RunStrategy: Send + Sync {
    async fn run(&self, messages: Vec<Message>) -> Result<String, AgentError>;
}
