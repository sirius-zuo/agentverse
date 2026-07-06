use agentverse::memory::MemoryError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct VectorRecord {
    pub user_id: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub importance: f32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct VectorHit {
    pub content: String,
    pub relevance: f32, // similarity in [0,1]; backends compute 1/(1+distance), where distance is COSINE distance for both built-in backends
    pub importance: f32,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn store(&self, record: VectorRecord) -> Result<(), MemoryError>;
    async fn search(
        &self,
        user_id: &str,
        embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorHit>, MemoryError>;
}

pub struct NoopVectorStore;

#[async_trait]
impl VectorStore for NoopVectorStore {
    async fn store(&self, _record: VectorRecord) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn search(
        &self,
        _user_id: &str,
        _embedding: &[f32],
        _top_k: usize,
    ) -> Result<Vec<VectorHit>, MemoryError> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_vector_store_roundtrip() {
        let s = NoopVectorStore;
        s.store(VectorRecord {
            user_id: "alice".into(),
            content: "x".into(),
            embedding: vec![0.1],
            importance: 0.5,
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
        assert!(s.search("alice", &[0.1], 5).await.unwrap().is_empty());
    }
}
