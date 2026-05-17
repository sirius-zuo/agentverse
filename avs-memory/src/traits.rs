use agentverse::memory::{MemoryError, Message};
use async_trait::async_trait;

#[async_trait]
pub trait Summarizer: Send + Sync {
    async fn summarize(&self, messages: &[Message]) -> Result<Message, MemoryError>;
}

pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryError>;
}

#[async_trait]
pub trait LongTermBackend: Send + Sync {
    async fn store(&self, message: Message, embedding: Vec<f32>) -> Result<(), MemoryError>;
    async fn search(&self, embedding: Vec<f32>, top_k: usize) -> Result<Vec<Message>, MemoryError>;
}
