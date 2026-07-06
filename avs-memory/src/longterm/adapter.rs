use std::sync::Arc;

use agentverse::memory::MemoryError;
use async_trait::async_trait;
use chrono::Utc;

use super::embedder::Embedder;
use super::vector::{VectorRecord, VectorStore};
use super::{LongtermMemory, LongtermRecord, ScoredMemory};

/// Weights for combining recency, importance, and relevance into a single score.
#[derive(Debug, Clone, Copy)]
pub struct ScoreWeights {
    /// α — weight applied to the recency component.
    pub recency: f32,
    /// β — weight applied to the importance component.
    pub importance: f32,
    /// γ — weight applied to the relevance (similarity) component.
    pub relevance: f32,
    /// Half-life used to decay recency over time.
    pub half_life: chrono::Duration,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            recency: 0.25,
            importance: 0.25,
            relevance: 0.5,
            half_life: chrono::Duration::days(7),
        }
    }
}

/// `LongtermMemory` implementation backed by an `Embedder` + `VectorStore`.
pub struct VectorLongtermMemory {
    embedder: Arc<dyn Embedder>,
    store: Arc<dyn VectorStore>,
    weights: ScoreWeights,
}

impl VectorLongtermMemory {
    pub fn new(embedder: Arc<dyn Embedder>, store: Arc<dyn VectorStore>) -> Self {
        Self {
            embedder,
            store,
            weights: ScoreWeights::default(),
        }
    }

    pub fn with_weights(mut self, weights: ScoreWeights) -> Self {
        self.weights = weights;
        self
    }
}

#[async_trait]
impl LongtermMemory for VectorLongtermMemory {
    async fn write(&self, user_id: &str, record: LongtermRecord) -> Result<(), MemoryError> {
        let embeddings = self
            .embedder
            .embed(std::slice::from_ref(&record.content))
            .await?;
        let mut it = embeddings.into_iter();
        let Some(embedding) = it.next() else {
            return Err(MemoryError::Embedding(
                "embedder returned no vectors".into(),
            ));
        };
        self.store
            .store(VectorRecord {
                user_id: user_id.to_string(),
                content: record.content,
                embedding,
                importance: record.importance,
                created_at: record.created_at,
            })
            .await
    }

    async fn retrieve(
        &self,
        user_id: &str,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<ScoredMemory>, MemoryError> {
        let embeddings = self.embedder.embed(&[query.to_string()]).await?;
        let mut it = embeddings.into_iter();
        let Some(embedding) = it.next() else {
            return Err(MemoryError::Embedding(
                "embedder returned no vectors".into(),
            ));
        };
        let hits = self.store.search(user_id, &embedding, top_k * 4).await?;

        let now = Utc::now();
        let half_life_seconds = self.weights.half_life.num_seconds() as f32;
        let mut scored: Vec<ScoredMemory> = hits
            .into_iter()
            .map(|hit| {
                let age_seconds = (now - hit.created_at).num_seconds() as f32;
                let recency = 0.5_f32.powf(age_seconds / half_life_seconds);
                let score = self.weights.recency * recency
                    + self.weights.importance * hit.importance
                    + self.weights.relevance * hit.relevance;
                ScoredMemory {
                    content: hit.content,
                    score,
                    created_at: hit.created_at,
                }
            })
            .collect();

        scored.sort_by(|a, b| b.score.total_cmp(&a.score));
        scored.truncate(top_k);
        Ok(scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::longterm::vector::VectorHit;

    struct FakeEmbedder {
        vector: Vec<f32>,
        should_fail: bool,
    }

    impl FakeEmbedder {
        fn ok(vector: Vec<f32>) -> Self {
            Self {
                vector,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                vector: vec![],
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl Embedder for FakeEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError> {
            if self.should_fail {
                return Err(MemoryError::Embedding("fake embedder failure".into()));
            }
            Ok(texts.iter().map(|_| self.vector.clone()).collect())
        }

        fn dimensions(&self) -> usize {
            self.vector.len()
        }
    }

    struct FakeStore {
        hits: Vec<VectorHit>,
        stored: Mutex<Vec<VectorRecord>>,
    }

    impl FakeStore {
        fn new(hits: Vec<VectorHit>) -> Self {
            Self {
                hits,
                stored: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl VectorStore for FakeStore {
        async fn store(&self, record: VectorRecord) -> Result<(), MemoryError> {
            self.stored.lock().unwrap().push(record);
            Ok(())
        }

        async fn search(
            &self,
            _user_id: &str,
            _embedding: &[f32],
            _top_k: usize,
        ) -> Result<Vec<VectorHit>, MemoryError> {
            Ok(self.hits.clone())
        }
    }

    #[tokio::test]
    async fn write_embeds_and_stores() {
        let embedder = Arc::new(FakeEmbedder::ok(vec![0.1, 0.2, 0.3]));
        let store = Arc::new(FakeStore::new(vec![]));
        let memory = VectorLongtermMemory::new(embedder, store.clone());

        let record = LongtermRecord {
            content: "hello".to_string(),
            importance: 0.9,
            created_at: Utc::now(),
        };
        memory.write("u", record).await.unwrap();

        let stored = store.stored.lock().unwrap();
        assert_eq!(stored.len(), 1);
        let captured = &stored[0];
        assert_eq!(captured.user_id, "u");
        assert_eq!(captured.content, "hello");
        assert_eq!(captured.embedding, vec![0.1, 0.2, 0.3]);
        assert!((captured.importance - 0.9).abs() < 1e-6);
    }

    #[tokio::test]
    async fn retrieve_scores_and_reorders() {
        let now = Utc::now();
        let hit_a = VectorHit {
            content: "A".to_string(),
            relevance: 0.9,
            importance: 0.0,
            created_at: now - chrono::Duration::days(30),
        };
        let hit_b = VectorHit {
            content: "B".to_string(),
            relevance: 0.7,
            importance: 1.0,
            created_at: now,
        };
        let embedder = Arc::new(FakeEmbedder::ok(vec![0.1]));
        let store = Arc::new(FakeStore::new(vec![hit_a, hit_b]));
        let memory = VectorLongtermMemory::new(embedder, store);

        let results = memory.retrieve("u", "query", 2).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].content, "B");
        assert_eq!(results[1].content, "A");
        assert!((results[0].score - 0.85).abs() < 1e-2);
        assert!((results[1].score - 0.463).abs() < 1e-2);
    }

    #[tokio::test]
    async fn embedder_error_propagates() {
        let embedder = Arc::new(FakeEmbedder::failing());
        let store = Arc::new(FakeStore::new(vec![]));
        let memory = VectorLongtermMemory::new(embedder, store);

        let result = memory.retrieve("u", "query", 2).await;

        assert!(result.is_err());
    }
}
