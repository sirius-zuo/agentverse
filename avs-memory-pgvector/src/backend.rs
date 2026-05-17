use agentverse::memory::{MemoryError, Message, MessageRole};
use agentverse_memory::LongTermBackend;
use sqlx::PgPool;
use sqlx::Row;

/// pgvector-backed long-term memory.
pub struct PgVectorBackend {
    pool: PgPool,
}

impl PgVectorBackend {
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

#[async_trait::async_trait]
impl LongTermBackend for PgVectorBackend {
    async fn store(&self, message: Message, embedding: Vec<f32>) -> Result<(), MemoryError> {
        let id = uuid::Uuid::new_v4();
        let content = message.content;
        let role = format!("{:?}", message.role);
        let metadata = serde_json::Value::Null;
        let created_at = chrono::Utc::now();

        // Build embedding vector string for pgvector
        let embedding_str = format!(
            "[{}]",
            embedding.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
        );

        sqlx::query(
            r#"
            INSERT INTO agent_memory (id, content, role, metadata, embedding, created_at)
            VALUES ($1, $2, $3, $4, $5::vector, $6)
            "#,
        )
        .bind(id)
        .bind(content)
        .bind(role)
        .bind(metadata)
        .bind(embedding_str)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| MemoryError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn search(&self, embedding: Vec<f32>, top_k: usize) -> Result<Vec<Message>, MemoryError> {
        let embedding_str = format!(
            "[{}]",
            embedding.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
        );

        let rows = sqlx::query(
            r#"
            SELECT content, role
            FROM agent_memory
            ORDER BY embedding <-> $1::vector
            LIMIT $2
            "#,
        )
        .bind(embedding_str)
        .bind(top_k as i32)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| MemoryError::Retrieval(e.to_string()))?;

        let mut messages = Vec::new();
        for row in rows {
            let content: String = row
                .try_get("content")
                .map_err(|e| MemoryError::Retrieval(e.to_string()))?;
            let role_str: String = row
                .try_get("role")
                .map_err(|e| MemoryError::Retrieval(e.to_string()))?;

            let role = match role_str.as_str() {
                "System" => MessageRole::System,
                "Assistant" => MessageRole::Assistant,
                "Tool" => MessageRole::Tool,
                _ => MessageRole::User,
            };

            messages.push(Message { role, content });
        }

        Ok(messages)
    }
}

impl PgVectorBackend {
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
