use agentverse::memory::MemoryError;
use agentverse_memory::{VectorHit, VectorRecord, VectorStore};
use sqlx::PgPool;
use sqlx::Row;

/// pgvector-backed, user-scoped long-term vector memory.
pub struct PgVectorStore {
    pool: PgPool,
}

impl PgVectorStore {
    pub async fn new(database_url: &str) -> Result<Self, MemoryError> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(|e| MemoryError::Storage(e.to_string()))?;

        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn embedding_to_vector_str(embedding: &[f32]) -> String {
    format!(
        "[{}]",
        embedding
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

#[async_trait::async_trait]
impl VectorStore for PgVectorStore {
    async fn store(&self, record: VectorRecord) -> Result<(), MemoryError> {
        let id = uuid::Uuid::new_v4();
        let embedding_str = embedding_to_vector_str(&record.embedding);

        sqlx::query(
            r#"
            INSERT INTO agent_memory (id, user_id, content, importance, embedding, created_at)
            VALUES ($1, $2, $3, $4, $5::vector, $6)
            "#,
        )
        .bind(id)
        .bind(record.user_id)
        .bind(record.content)
        .bind(record.importance)
        .bind(embedding_str)
        .bind(record.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| MemoryError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn search(
        &self,
        user_id: &str,
        embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorHit>, MemoryError> {
        let embedding_str = embedding_to_vector_str(embedding);

        let rows = sqlx::query(
            r#"
            SELECT content, importance, created_at, (embedding <=> $2::vector) AS distance
            FROM agent_memory
            WHERE user_id = $1
            ORDER BY embedding <=> $2::vector
            LIMIT $3
            "#,
        )
        .bind(user_id)
        .bind(embedding_str)
        .bind(top_k as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MemoryError::Retrieval(e.to_string()))?;

        let mut hits = Vec::new();
        for row in rows {
            let content: String = row
                .try_get("content")
                .map_err(|e| MemoryError::Retrieval(e.to_string()))?;
            let importance: f32 = row
                .try_get("importance")
                .map_err(|e| MemoryError::Retrieval(e.to_string()))?;
            let created_at = row
                .try_get("created_at")
                .map_err(|e| MemoryError::Retrieval(e.to_string()))?;
            let distance: f64 = row
                .try_get("distance")
                .map_err(|e| MemoryError::Retrieval(e.to_string()))?;

            hits.push(VectorHit {
                content,
                relevance: 1.0 / (1.0 + distance as f32),
                importance,
                created_at,
            });
        }

        Ok(hits)
    }
}

impl PgVectorStore {
    pub async fn purge_old(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<usize, MemoryError> {
        let result = sqlx::query(r#"DELETE FROM agent_memory WHERE created_at < $1"#)
            .bind(before)
            .execute(&self.pool)
            .await
            .map_err(|e| MemoryError::Storage(e.to_string()))?;

        Ok(result.rows_affected() as usize)
    }

    pub async fn health_check(&self) -> Result<(), MemoryError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| MemoryError::Storage(e.to_string()))?;
        Ok(())
    }
}
