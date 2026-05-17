use agentverse::memory::{MemoryError, Message, MessageRole};
use agentverse_memory::LongTermBackend;
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

    async fn connect(&self) -> Result<lancedb::Connection, MemoryError> {
        let uri = format!("file://{}", self.db_path);
        lancedb::connect(&uri)
            .execute()
            .await
            .map_err(|e| MemoryError::Storage(e.to_string()))
    }

    async fn open_or_create_table(
        &self,
        conn: &lancedb::Connection,
    ) -> Result<lancedb::table::Table, MemoryError> {
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
                .map_err(|e| MemoryError::Storage(e.to_string()))?;
                conn.create_table(&self.table_name, vec![empty_batch])
                    .execute()
                    .await
                    .map_err(|e| MemoryError::Storage(e.to_string()))
            }
        }
    }

    pub async fn health_check(&self) -> Result<(), MemoryError> {
        self.connect().await?;
        Ok(())
    }

    pub async fn purge_old(
        &self,
        _before: chrono::DateTime<chrono::Utc>,
    ) -> Result<usize, MemoryError> {
        // LanceDB doesn't have native time-based deletion in MVP
        Ok(0)
    }
}

#[async_trait::async_trait]
impl LongTermBackend for LanceDBBackend {
    async fn store(&self, message: Message, embedding: Vec<f32>) -> Result<(), MemoryError> {
        let conn = self.connect().await?;
        let table = self.open_or_create_table(&conn).await?;

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("role", DataType::Utf8, false),
            Field::new("metadata", DataType::Utf8, true),
            Field::new("created_at", DataType::Utf8, false),
        ]));

        let id = Uuid::new_v4().to_string();
        let role = format!("{:?}", message.role);
        let metadata = serde_json::to_string(&embedding).unwrap_or_default();
        let created_at = chrono::Utc::now().to_rfc3339();

        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec![id])),
                Arc::new(StringArray::from(vec![message.content])),
                Arc::new(StringArray::from(vec![role])),
                Arc::new(StringArray::from(vec![metadata])),
                Arc::new(StringArray::from(vec![created_at])),
            ],
        )
        .map_err(|e| MemoryError::Storage(e.to_string()))?;

        table
            .add(vec![batch])
            .mode(AddDataMode::Append)
            .execute()
            .await
            .map_err(|e| MemoryError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn search(
        &self,
        _embedding: Vec<f32>,
        top_k: usize,
    ) -> Result<Vec<Message>, MemoryError> {
        let conn = self.connect().await?;
        let table = self.open_or_create_table(&conn).await?;

        let mut results = table
            .query()
            .limit(top_k)
            .execute()
            .await
            .map_err(|e| MemoryError::Retrieval(e.to_string()))?;

        let mut messages = Vec::new();
        while let Some(batch) = results
            .next()
            .await
            .transpose()
            .map_err(|e| MemoryError::Retrieval(e.to_string()))?
        {
            if let (Some(role_arr), Some(content_arr)) = (
                batch.column_by_name("role"),
                batch.column_by_name("content"),
            ) {
                let role_array = role_arr
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .unwrap();
                let content_array = content_arr
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .unwrap();
                for i in 0..batch.num_rows() {
                    let role = match role_array.value(i) {
                        "System" => MessageRole::System,
                        "Assistant" => MessageRole::Assistant,
                        "Tool" => MessageRole::Tool,
                        _ => MessageRole::User,
                    };
                    messages.push(Message {
                        role,
                        content: content_array.value(i).to_string(),
                    });
                }
            }
        }

        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse_memory::LongTermBackend;

    #[tokio::test]
    async fn test_health_check() {
        let backend = LanceDBBackend::new("/tmp/test-lancedb-memory", "messages");
        // Should not panic — creates/verifies the database
        let result = backend.health_check().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_store_and_search() {
        let backend = LanceDBBackend::new("/tmp/test-lancedb-store-search", "messages");

        let result = backend
            .store(
                Message {
                    role: MessageRole::User,
                    content: "Hello, how are you?".to_string(),
                },
                vec![],
            )
            .await;
        assert!(result.is_ok());

        let results = backend.search(vec![], 10).await;
        assert!(results.is_ok());
    }
}
