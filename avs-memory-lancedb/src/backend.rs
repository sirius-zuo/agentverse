use agentverse::{Message, MessageRole};
use agentverse_memory::{LongTermMemory, LongTermMemoryError, MemoryEntry};
use arrow_array::{RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use futures_util::stream::StreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::table::AddDataMode;
use std::sync::Arc;
use uuid::Uuid;

/// LanceDB-backed long-term memory.
/// Stores messages as vector records with metadata.
pub struct LanceDBBackend {
    db_path: String,
    table_name: String,
}

impl LanceDBBackend {
    pub fn new(db_path: &str, table_name: &str) -> Self {
        Self {
            db_path: db_path.to_string(),
            table_name: table_name.to_string(),
        }
    }

    async fn connect(&self) -> Result<lancedb::Connection, LongTermMemoryError> {
        let uri = format!("file://{}", self.db_path);
        lancedb::connect(&uri)
            .execute()
            .await
            .map_err(|e| LongTermMemoryError::Connection(e.to_string()))
    }

    async fn open_or_create_table(
        &self,
        conn: &lancedb::Connection,
    ) -> Result<lancedb::table::Table, LongTermMemoryError> {
        // Try to open existing table, or create if it doesn't exist
        match conn.open_table(&self.table_name).execute().await {
            Ok(table) => Ok(table),
            Err(_) => {
                // Create an empty table with the schema
                let schema = Arc::new(Schema::new(vec![
                    Field::new("id", DataType::Utf8, false),
                    Field::new("content", DataType::Utf8, false),
                    Field::new("role", DataType::Utf8, false),
                    Field::new("metadata", DataType::Utf8, true),
                    Field::new("created_at", DataType::Utf8, false),
                ]));
                let empty_batch = RecordBatch::try_new(
                    schema.clone(),
                    vec![
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                        Arc::new(StringArray::from(Vec::<&str>::new())),
                    ],
                )
                .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;
                conn.create_table(&self.table_name, vec![empty_batch])
                    .execute()
                    .await
                    .map_err(|e| LongTermMemoryError::Connection(e.to_string()))
            }
        }
    }
}

#[async_trait::async_trait]
impl LongTermMemory for LanceDBBackend {
    async fn store(&mut self, entry: MemoryEntry) -> Result<(), LongTermMemoryError> {
        let conn = self.connect().await?;
        let table = self.open_or_create_table(&conn).await?;

        // Create a schema matching the table columns
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("role", DataType::Utf8, false),
            Field::new("metadata", DataType::Utf8, true),
            Field::new("created_at", DataType::Utf8, false),
        ]));

        let id = Uuid::new_v4().to_string();
        let content = format!("{:?}: {}", entry.message.role, entry.message.content);
        let role = format!("{:?}", entry.message.role);
        let metadata = serde_json::to_string(&entry.metadata).unwrap_or_default();
        let created_at = entry.created_at.to_rfc3339();

        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec![id.clone()])),
                Arc::new(StringArray::from(vec![content])),
                Arc::new(StringArray::from(vec![role])),
                Arc::new(StringArray::from(vec![metadata])),
                Arc::new(StringArray::from(vec![created_at])),
            ],
        )
        .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        table
            .add(vec![batch])
            .mode(AddDataMode::Append)
            .execute()
            .await
            .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        Ok(())
    }

    async fn search(
        &self,
        _query: &str,
        top_k: usize,
    ) -> Result<Vec<MemoryEntry>, LongTermMemoryError> {
        let conn = self.connect().await?;
        let table = self.open_or_create_table(&conn).await?;

        // LanceDB full-text search (simplified — no actual embedding)
        let mut results = table
            .query()
            .limit(top_k)
            .execute()
            .await
            .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        let mut entries = Vec::new();
        while let Some(batch) = results
            .next()
            .await
            .transpose()
            .map_err(|e| LongTermMemoryError::Query(e.to_string()))?
        {
            if let (Some(id_arr), Some(content_arr)) =
                (batch.column_by_name("id"), batch.column_by_name("content"))
            {
                let id_array = id_arr
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .unwrap();
                let content_array = content_arr
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .unwrap();
                for i in 0..batch.num_rows() {
                    entries.push(MemoryEntry {
                        id: id_array.value(i).to_string(),
                        message: Message {
                            role: MessageRole::User,
                            content: content_array.value(i).to_string(),
                        },
                        metadata: serde_json::Value::Null,
                        created_at: chrono::Utc::now(),
                    });
                }
            }
        }

        Ok(entries)
    }

    async fn purge_old(
        &mut self,
        _before: chrono::DateTime<chrono::Utc>,
    ) -> Result<usize, LongTermMemoryError> {
        // LanceDB doesn't have native time-based deletion in MVP
        // Implement via query filter in production
        Ok(0)
    }

    async fn health_check(&self) -> Result<(), LongTermMemoryError> {
        self.connect().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse_memory::LongTermMemory;

    #[tokio::test]
    async fn test_health_check() {
        let backend = LanceDBBackend::new("/tmp/test-lancedb-memory", "messages");
        // Should not panic — creates/verifies the database
        let result = backend.health_check().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_store_and_search() {
        let mut backend = LanceDBBackend::new("/tmp/test-lancedb-store-search", "messages");

        let entry = MemoryEntry {
            id: Uuid::new_v4().to_string(),
            message: Message {
                role: MessageRole::User,
                content: "Hello, how are you?".to_string(),
            },
            metadata: serde_json::json!({"conversation_id": "123"}),
            created_at: chrono::Utc::now(),
        };

        let result = backend.store(entry.clone()).await;
        assert!(result.is_ok());

        let results = backend.search("hello", 10).await;
        assert!(results.is_ok());
    }
}
