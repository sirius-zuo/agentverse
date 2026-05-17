use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("Summarization failed: {0}")]
    Summarization(String),
    #[error("Storage failed: {0}")]
    Storage(String),
    #[error("Retrieval failed: {0}")]
    Retrieval(String),
}

#[async_trait]
pub trait Memory: Send + Sync {
    fn append(&mut self, message: Message);
    async fn last_n(&mut self, n: usize) -> Result<Vec<Message>, MemoryError>;
    fn pin(&mut self, messages: Vec<Message>);
    async fn prime_from_long_term(&mut self, query: &str, top_k: usize) -> Result<(), MemoryError>;
    async fn flush(&mut self) -> Result<(), MemoryError>;
    fn clear(&mut self);
}

mod short_term;
pub(crate) use short_term::ShortTermMemory;
