use agentverse::Message;
use agentverse_memory::{LongTermMemory, LongTermMemoryError, MemoryEntry};
use sqlx::Row;
use sqlx::PgPool;

/// pgvector-backed long-term memory.
pub struct PgVectorBackend {
    pool: PgPool,
}

impl PgVectorBackend {
    pub async fn new(database_url: &str) -> Result<Self, LongTermMemoryError> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(|e| LongTermMemoryError::Connection(e.to_string()))?;

        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl LongTermMemory for PgVectorBackend {
    async fn store(&mut self, entry: MemoryEntry) -> Result<(), LongTermMemoryError> {
        let id = entry.id.parse::<uuid::Uuid>().map_err(|e| LongTermMemoryError::Query(e.to_string()))?;
        let content = format!("{:?}: {}", entry.message.role, entry.message.content);
        let role = format!("{:?}", entry.message.role);
        let metadata = serde_json::to_value(&entry.metadata).map_err(|e| LongTermMemoryError::Query(e.to_string()))?;
        let created_at = entry.created_at;

        // Build embedding array string for pgvector
        let embedding = format!("[{}]", vec![0.0f32; 1536].iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));

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
        .bind(embedding)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        Ok(())
    }

    async fn search(
        &self,
        _query: &str,
        top_k: usize,
    ) -> Result<Vec<MemoryEntry>, LongTermMemoryError> {
        // Build placeholder embedding for vector similarity search
        let embedding = format!("[{}]", vec![0.0f32; 1536].iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));

        let rows = sqlx::query(
            r#"
            SELECT id, content, role, metadata, created_at
            FROM agent_memory
            ORDER BY embedding <-> $1::vector
            LIMIT $2
            "#,
        )
        .bind(embedding)
        .bind(top_k as i32)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            let id: uuid::Uuid = row.try_get("id").map_err(|e| LongTermMemoryError::Query(e.to_string()))?;
            let content: String = row.try_get("content").map_err(|e| LongTermMemoryError::Query(e.to_string()))?;
            let role: String = row.try_get("role").map_err(|e| LongTermMemoryError::Query(e.to_string()))?;
            let metadata: serde_json::Value = row.try_get("metadata").unwrap_or(serde_json::Value::Null);
            let created_at: chrono::DateTime<chrono::Utc> = row.try_get("created_at").map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

            let message_role = match role.as_str() {
                "System" => agentverse::MessageRole::System,
                "User" => agentverse::MessageRole::User,
                "Assistant" => agentverse::MessageRole::Assistant,
                "Tool" => agentverse::MessageRole::Tool,
                _ => agentverse::MessageRole::User,
            };

            entries.push(MemoryEntry {
                id: id.to_string(),
                message: Message {
                    role: message_role,
                    content,
                },
                metadata,
                created_at,
            });
        }

        Ok(entries)
    }

    async fn purge_old(&mut self, before: chrono::DateTime<chrono::Utc>) -> Result<usize, LongTermMemoryError> {
        let result = sqlx::query(
            r#"DELETE FROM agent_memory WHERE created_at < $1"#,
        )
        .bind(before)
        .execute(&self.pool)
        .await
        .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        Ok(result.rows_affected() as usize)
    }

    async fn health_check(&self) -> Result<(), LongTermMemoryError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| LongTermMemoryError::Connection(e.to_string()))?;
        Ok(())
    }
}
