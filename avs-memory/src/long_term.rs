use agentverse::Message;
use serde::{Deserialize, Serialize};

/// A stored memory entry with embedding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub message: Message,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Trait for long-term memory backends.
/// Implementations: LanceDB, pgvector, etc.
#[async_trait::async_trait]
pub trait LongTermMemory: Send + Sync {
    /// Store a message with its embedding.
    async fn store(&mut self, entry: MemoryEntry) -> Result<(), LongTermMemoryError>;

    /// Search for similar messages by semantic similarity.
    async fn search(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<MemoryEntry>, LongTermMemoryError>;

    /// Delete entries older than a timestamp.
    async fn purge_old(&mut self, before: chrono::DateTime<chrono::Utc>) -> Result<usize, LongTermMemoryError>;

    /// Check if the backend is healthy.
    async fn health_check(&self) -> Result<(), LongTermMemoryError>;
}

#[derive(thiserror::Error, Debug)]
pub enum LongTermMemoryError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Query error: {0}")]
    Query(String),
    #[error("Embedding error: {0}")]
    Embedding(String),
    #[error("Not found: {0}")]
    NotFound(String),
}
