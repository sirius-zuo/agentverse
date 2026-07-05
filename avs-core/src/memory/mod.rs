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
