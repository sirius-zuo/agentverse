use async_trait::async_trait;
use chrono::{DateTime, Utc};
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
    async fn flush(&mut self) -> Result<(), MemoryError>;
    fn clear(&mut self);
}

#[derive(Debug, Clone)]
pub struct LongtermRecord {
    pub content: String,
    /// LLM-assigned or heuristic importance score, 0.0–1.0.
    pub importance: f32,
    pub created_at: DateTime<Utc>,
}

impl LongtermRecord {
    pub fn now(content: String, importance: f32) -> Self {
        let clamped = if importance.is_nan() {
            0.0
        } else {
            importance.clamp(0.0, 1.0)
        };
        if clamped != importance {
            tracing::warn!(
                importance,
                clamped,
                "LongtermRecord importance outside [0.0, 1.0]; clamping"
            );
        }
        Self {
            content,
            importance: clamped,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScoredMemory {
    pub content: String,
    /// Combined score: α·recency + β·importance + γ·relevance
    pub score: f32,
    pub created_at: DateTime<Utc>,
}

/// Layer 3 user-scoped long-term store. See also `LongTermBackend` for the lower-level embedding interface.
#[async_trait]
pub trait LongtermMemory: Send + Sync {
    async fn write(&self, user_id: &str, record: LongtermRecord) -> Result<(), MemoryError>;
    async fn retrieve(
        &self,
        user_id: &str,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<ScoredMemory>, MemoryError>;
}

mod short_term;

#[cfg(test)]
mod store_tests {
    use super::*;

    struct NoopMemoryStore;

    #[async_trait]
    impl LongtermMemory for NoopMemoryStore {
        async fn write(&self, _: &str, _: LongtermRecord) -> Result<(), MemoryError> {
            Ok(())
        }
        async fn retrieve(
            &self,
            _: &str,
            _: &str,
            _: usize,
        ) -> Result<Vec<ScoredMemory>, MemoryError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn noop_memory_store_retrieve_returns_empty() {
        let store = NoopMemoryStore;
        let result = store.retrieve("alice", "test query", 5).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn long_term_record_now_sets_fields() {
        let r = LongtermRecord::now("hello".to_string(), 0.7);
        assert_eq!(r.content, "hello");
        assert!((r.importance - 0.7).abs() < 1e-6);
    }

    #[test]
    fn importance_above_one_is_clamped() {
        assert_eq!(LongtermRecord::now("x".into(), 1.5).importance, 1.0);
    }

    #[test]
    fn importance_below_zero_is_clamped() {
        assert_eq!(LongtermRecord::now("x".into(), -0.3).importance, 0.0);
    }

    #[test]
    fn importance_nan_becomes_zero() {
        assert_eq!(LongtermRecord::now("x".into(), f32::NAN).importance, 0.0);
    }

    #[test]
    fn importance_in_range_is_untouched() {
        assert_eq!(LongtermRecord::now("x".into(), 0.7).importance, 0.7);
    }
}
