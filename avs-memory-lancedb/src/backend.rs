use agentverse::memory::MemoryError;
use agentverse_memory::{VectorHit, VectorRecord, VectorStore};
use arrow_array::{Array, FixedSizeListArray};
use arrow_array::{Float32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use chrono::DateTime;
use futures_util::stream::StreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::table::AddDataMode;
use std::sync::Arc;
use uuid::Uuid;

fn schema(dims: usize) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("user_id", DataType::Utf8, false),
        Field::new("content", DataType::Utf8, false),
        Field::new("importance", DataType::Float32, false),
        Field::new("created_at", DataType::Utf8, false), // rfc3339
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dims as i32,
            ),
            false,
        ),
    ]))
}

/// LanceDB-backed, user-scoped long-term vector memory with real ANN search.
pub struct LanceDbVectorStore {
    db_path: String,
    table_name: String,
    dimensions: usize,
}

impl LanceDbVectorStore {
    pub fn new(db_path: &str, table_name: &str, dimensions: usize) -> Self {
        Self {
            db_path: db_path.to_string(),
            table_name: table_name.to_string(),
            dimensions,
        }
    }

    async fn connect(&self) -> Result<lancedb::Connection, MemoryError> {
        lancedb::connect(&self.db_path)
            .execute()
            .await
            .map_err(|e| MemoryError::Storage(e.to_string()))
    }

    async fn open_or_create_table(
        &self,
        conn: &lancedb::Connection,
    ) -> Result<lancedb::table::Table, MemoryError> {
        match conn.open_table(&self.table_name).execute().await {
            Ok(table) => Ok(table),
            Err(_) => conn
                .create_empty_table(&self.table_name, schema(self.dimensions))
                .execute()
                .await
                .map_err(|e| MemoryError::Storage(e.to_string())),
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
        // LanceDB time-based deletion is a follow-up ticket.
        Ok(0)
    }
}

#[async_trait::async_trait]
impl VectorStore for LanceDbVectorStore {
    async fn store(&self, record: VectorRecord) -> Result<(), MemoryError> {
        if record.embedding.len() != self.dimensions {
            return Err(MemoryError::Storage(format!(
                "embedding has {} dimensions, expected {}",
                record.embedding.len(),
                self.dimensions
            )));
        }

        let conn = self.connect().await?;
        let table = self.open_or_create_table(&conn).await?;

        let id = Uuid::new_v4().to_string();
        let created_at = record.created_at.to_rfc3339();

        let embedding_values = Float32Array::from(record.embedding);
        let embedding_array = FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            self.dimensions as i32,
            Arc::new(embedding_values),
            None,
        )
        .map_err(|e| MemoryError::Storage(e.to_string()))?;

        let batch = RecordBatch::try_new(
            schema(self.dimensions),
            vec![
                Arc::new(StringArray::from(vec![id])),
                Arc::new(StringArray::from(vec![record.user_id])),
                Arc::new(StringArray::from(vec![record.content])),
                Arc::new(Float32Array::from(vec![record.importance])),
                Arc::new(StringArray::from(vec![created_at])),
                Arc::new(embedding_array),
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
        user_id: &str,
        embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorHit>, MemoryError> {
        let conn = self.connect().await?;
        let table = self.open_or_create_table(&conn).await?;

        let mut results = table
            .query()
            .nearest_to(embedding)
            .map_err(|e| MemoryError::Retrieval(e.to_string()))?
            .distance_type(lancedb::DistanceType::Cosine)
            .only_if(format!("user_id = '{}'", user_id.replace('\'', "''")))
            .limit(top_k)
            .execute()
            .await
            .map_err(|e| MemoryError::Retrieval(e.to_string()))?;

        let mut hits = Vec::new();
        while let Some(batch) = results
            .next()
            .await
            .transpose()
            .map_err(|e| MemoryError::Retrieval(e.to_string()))?
        {
            let content_array = batch
                .column_by_name("content")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                .ok_or_else(|| MemoryError::Retrieval("missing content column".to_string()))?;
            let importance_array = batch
                .column_by_name("importance")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>())
                .ok_or_else(|| MemoryError::Retrieval("missing importance column".to_string()))?;
            let created_at_array = batch
                .column_by_name("created_at")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                .ok_or_else(|| MemoryError::Retrieval("missing created_at column".to_string()))?;
            let distance_array = batch
                .column_by_name("_distance")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>())
                .ok_or_else(|| MemoryError::Retrieval("missing _distance column".to_string()))?;

            for i in 0..batch.num_rows() {
                let created_at = DateTime::parse_from_rfc3339(created_at_array.value(i))
                    .map_err(|e| MemoryError::Retrieval(e.to_string()))?
                    .with_timezone(&chrono::Utc);
                let distance = distance_array.value(i);

                hits.push(VectorHit {
                    content: content_array.value(i).to_string(),
                    relevance: 1.0 / (1.0 + distance),
                    importance: importance_array.value(i),
                    created_at,
                });
            }
        }

        hits.sort_by(|a, b| b.relevance.total_cmp(&a.relevance));
        hits.truncate(top_k);

        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse_memory::VectorRecord;

    #[tokio::test]
    async fn search_returns_nearest_and_is_user_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let store = LanceDbVectorStore::new(dir.path().to_str().unwrap(), "mem", 3);
        let now = chrono::Utc::now();
        let rec = |user: &str, content: &str, emb: Vec<f32>| VectorRecord {
            user_id: user.into(),
            content: content.into(),
            embedding: emb,
            importance: 0.5,
            created_at: now,
        };
        store
            .store(rec("alice", "far", vec![10.0, 10.0, 10.0]))
            .await
            .unwrap();
        store
            .store(rec("alice", "near", vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .store(rec("bob", "nearest-but-bob", vec![0.9, 0.0, 0.0]))
            .await
            .unwrap();
        let hits = store.search("alice", &[1.0, 0.0, 0.0], 2).await.unwrap();
        assert_eq!(hits[0].content, "near"); // old code returned insertion order
        assert!(hits.iter().all(|h| h.content != "nearest-but-bob"));
    }

    #[tokio::test]
    async fn health_check_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let store = LanceDbVectorStore::new(dir.path().to_str().unwrap(), "mem", 3);
        assert!(store.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn store_rejects_wrong_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let store = LanceDbVectorStore::new(dir.path().to_str().unwrap(), "mem", 3);
        let rec = VectorRecord {
            user_id: "alice".into(),
            content: "bad".into(),
            embedding: vec![1.0, 2.0],
            importance: 0.5,
            created_at: chrono::Utc::now(),
        };
        assert!(store.store(rec).await.is_err());
    }
}
