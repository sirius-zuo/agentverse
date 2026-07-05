use agentverse::memory::{MemoryError, Message};
use async_trait::async_trait;

#[async_trait]
pub trait LongTermBackend: Send + Sync {
    async fn store(&self, message: Message, embedding: Vec<f32>) -> Result<(), MemoryError>;
    async fn search(&self, embedding: Vec<f32>, top_k: usize) -> Result<Vec<Message>, MemoryError>;
}
