# Phase 3: Memory System

> **Goal:** Implement layered memory system with LanceDB and pgvector backends.
> **Dependencies:** Phase 1 (avs-core) must be complete
> **Parallel:** All 3 crates can develop in parallel

---

## Overview

Memory is layered:
- **Short-term**: In-memory `Vec<Message>` per user, bounded by `max_messages`
- **Long-term**: Pluggable vector DB backend for semantic search across conversations

```
ShortTermMemory (Vec<Message>)
    └── last_n(n) → recent messages for prompt context
    └── auto-summary trigger → summarize old messages

LongTermMemory (trait)
    ├── LanceDBBackend (embedded, file-based)
    └── PgVectorBackend (PostgreSQL extension)
    └── store(message, embedding)
    └── search(query, top_k) → similar messages
```

## File Structure

```
avs-memory/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── long_term.rs      # LongTermMemory trait
│   └── summary.rs        # Auto-summary utilities
│
avs-memory-lancedb/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   └── backend.rs        # LanceDB implementation
│
avs-memory-pgvector/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   └── backend.rs        # pgvector implementation
│   └── migration.rs      # SQL migration scripts
```

---

## Task 1: avs-memory — LongTermMemory trait + summary

**Files:**
- Create: `avs-memory/Cargo.toml`
- Create: `avs-memory/src/lib.rs`
- Create: `avs-memory/src/long_term.rs`
- Create: `avs-memory/src/summary.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-memory"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
serde.workspace = true
serde_json.workspace = true
tracing.workspace = true
```

- [ ] **Step 2: long_term.rs — Trait definition**

```rust
// avs-memory/src/long_term.rs
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
```

- [ ] **Step 3: summary.rs — Auto-summary logic**

```rust
// avs-memory/src/summary.rs
use agentverse::Message;

/// Summarize a list of messages into a shorter form.
/// This is a local function — the actual summary generation
/// should be done by the LLM. This provides a fallback truncation.
pub fn truncate_messages(messages: &[Message], max_tokens: usize) -> String {
    let mut result = String::new();
    let mut token_count = 0;

    // Keep most recent messages
    for msg in messages.iter().rev() {
        let msg_str = format!("{}: {}\n", msg.role, msg.content);
        let chars = msg_str.chars().count();
        if token_count + chars > max_tokens {
            break;
        }
        result.insert_str(0, &msg_str);
        token_count += chars;
    }

    if token_count > 0 {
        result.insert_str(0, "[Summary of older messages]\n");
    }

    result
}

/// Detect if auto-summary should be triggered.
/// Default threshold: 50 messages
pub fn should_trigger_summary(message_count: usize, threshold: usize) -> bool {
    message_count >= threshold
}
```

- [ ] **Step 4: lib.rs**

```rust
// avs-memory/src/lib.rs
pub mod long_term;
pub mod summary;

pub use long_term::{LongTermMemory, LongTermMemoryError, MemoryEntry};
pub use summary::{should_trigger_summary, truncate_messages};
```

- [ ] **Step 5: Verify + commit**

Run: `cargo check -p agentverse-memory`
Run: `cargo test -p agentverse-memory`
Commit: `git add avs-memory/ && git commit -m "feat: add memory system with LongTermMemory trait"`

---

## Task 2: avs-memory-lancedb — LanceDB Backend

**Files:**
- Create: `avs-memory-lancedb/Cargo.toml`
- Create: `avs-memory-lancedb/src/lib.rs`
- Create: `avs-memory-lancedb/src/backend.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-memory-lancedb"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
agentverse-memory = { path = "../avs-memory" }
lancedb = "0.8"
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
uuid.workspace = true
```

- [ ] **Step 2: backend.rs**

```rust
// avs-memory-lancedb/src/backend.rs
use agentverse::Message;
use agentverse_memory::{LongTermMemory, LongTermMemoryError, MemoryEntry};
use lancedb::connection::ConnectionBuilder;
use lancedb::table::WriteMode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// LanceDB-backed long-term memory.
/// Stores messages as vector records with metadata.
pub struct LanceDBBackend {
    db_path: String,
    table_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct LanceRecord {
    id: String,
    content: String,
    role: String,
    metadata: Option<String>,
    created_at: String,
}

impl LanceDBBackend {
    pub fn new(db_path: &str, table_name: &str) -> Self {
        Self {
            db_path: db_path.to_string(),
            table_name: table_name.to_string(),
        }
    }

    async fn connect(&self) -> Result<lancedb::Connection, LongTermMemoryError> {
        let conn = ConnectionBuilder::try_from_uri(&format!("file://{}", self.db_path))
            .await
            .map_err(|e| LongTermMemoryError::Connection(e.to_string()))?;
        Ok(conn)
    }
}

#[async_trait::async_trait]
impl LongTermMemory for LanceDBBackend {
    async fn store(&mut self, entry: MemoryEntry) -> Result<(), LongTermMemoryError> {
        let conn = self.connect().await?;
        let mut table = conn
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| LongTermMemoryError::Connection(e.to_string()))?;

        // Convert to LanceDB-compatible record
        let record = LanceRecord {
            id: entry.id,
            content: format!("{}: {}", entry.message.role, entry.message.content),
            role: format!("{}", entry.message.role),
            metadata: Some(serde_json::to_string(&entry.metadata).ok()),
            created_at: entry.created_at.to_rfc3339(),
        };

        // LanceDB requires embedding column for search
        // For MVP, we use a placeholder embedding (zero vector)
        // In production, use an actual embedding model
        let records = vec![record];
        table
            .add(records.into_iter())
            .write_mode(WriteMode::Overwrite)
            .execute()
            .await
            .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        Ok(())
    }

    async fn search(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<MemoryEntry>, LongTermMemoryError> {
        let conn = self.connect().await?;
        let table = conn
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| LongTermMemoryError::Connection(e.to_string()))?;

        // LanceDB vector search (simplified — no actual embedding)
        // In production: query.vector(&embedding, top_k)
        let results = table
            .search(query)
            .limit(top_k)
            .execute()
            .await
            .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        let mut entries = Vec::new();
        for row in results {
            if let Some(id) = row.get("id").and_then(|v| v.as_str()) {
                if let Some(content) = row.get("content").and_then(|v| v.as_str()) {
                    entries.push(MemoryEntry {
                        id: id.to_string(),
                        message: Message {
                            role: agentverse::MessageRole::User, // simplified
                            content: content.to_string(),
                        },
                        metadata: serde_json::Value::Null,
                        created_at: chrono::Utc::now(),
                    });
                }
            }
        }

        Ok(entries)
    }

    async fn purge_old(&mut self, before: chrono::DateTime<chrono::Utc>) -> Result<usize, LongTermMemoryError> {
        // LanceDB doesn't have native time-based deletion in MVP
        // Implement via query filter in production
        Ok(0)
    }

    async fn health_check(&self) -> Result<(), LongTermMemoryError> {
        self.connect().await?;
        Ok(())
    }
}
```

- [ ] **Step 3: lib.rs**

```rust
// avs-memory-lancedb/src/lib.rs
mod backend;
pub use backend::LanceDBBackend;
```

- [ ] **Step 4: Verify + commit**

Run: `cargo check -p agentverse-memory-lancedb`
Run: `cargo test -p agentverse-memory-lancedb`
Commit: `git add avs-memory-lancedb/ && git commit -m "feat: add LanceDB memory backend"`

---

## Task 3: avs-memory-pgvector — pgvector Backend

**Files:**
- Create: `avs-memory-pgvector/Cargo.toml`
- Create: `avs-memory-pgvector/src/lib.rs`
- Create: `avs-memory-pgvector/src/backend.rs`
- Create: `avs-memory-pgvector/src/migration.sql`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-memory-pgvector"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
agentverse-memory = { path = "../avs-memory" }
sqlx = { version = "0.7", features = ["runtime-tokio-rustls", "postgres", "uuid", "chrono"] }
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
uuid.workspace = true
```

- [ ] **Step 2: migration.sql**

```sql
-- avs-memory-pgvector/src/migration.sql
-- Requires: CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS agent_memory (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    content TEXT NOT NULL,
    role VARCHAR(20) NOT NULL,
    metadata JSONB,
    embedding vector(1536),  -- OpenAI ada-002 dimension
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_memory_embedding ON agent_memory USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
```

- [ ] **Step 3: backend.rs**

```rust
// avs-memory-pgvector/src/backend.rs
use agentverse::Message;
use agentverse_memory::{LongTermMemory, LongTermMemoryError, MemoryEntry};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

/// pgvector-backed long-term memory.
pub struct PgVectorBackend {
    pool: PgPool,
}

impl PgVectorBackend {
    pub fn new(database_url: &str) -> Result<Self, LongTermMemoryError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .block_on()
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
        sqlx::query!(
            r#"
            INSERT INTO agent_memory (id, content, role, metadata, embedding, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            entry.id.parse::<uuid::Uuid>().ok(),
            format!("{}: {}", entry.message.role, entry.message.content),
            format!("{}", entry.message.role),
            serde_json::to_value(&entry.metadata).ok(),
            // Embedding as [0.0, 0.0, ...] — real embedding from model
            &[0.0f32; 1536],
            entry.created_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        Ok(())
    }

    async fn search(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<MemoryEntry>, LongTermMemoryError> {
        // Vector similarity search using pgvector's <-> operator
        let query_embedding = &[0.0f32; 1536]; // real embedding in production

        let rows = sqlx::query!(
            r#"
            SELECT id, content, role, metadata, created_at
            FROM agent_memory
            ORDER BY embedding <-> $1::vector
            LIMIT $2
            "#,
            query_embedding,
            top_k as i32,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LongTermMemoryError::Query(e.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(MemoryEntry {
                id: row.id.to_string(),
                message: Message {
                    role: agentverse::MessageRole::User,
                    content: row.content,
                },
                metadata: row.metadata.unwrap_or_default(),
                created_at: row.created_at,
            });
        }

        Ok(entries)
    }

    async fn purge_old(&mut self, before: chrono::DateTime<chrono::Utc>) -> Result<usize, LongTermMemoryError> {
        let result = sqlx::query!(
            r#"DELETE FROM agent_memory WHERE created_at < $1"#,
            before,
        )
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
```

- [ ] **Step 4: lib.rs**

```rust
// avs-memory-pgvector/src/lib.rs
mod backend;
pub use backend::PgVectorBackend;
```

- [ ] **Step 5: Verify + commit**

Run: `cargo check -p agentverse-memory-pgvector`
Run: `cargo test -p agentverse-memory-pgvector`
Commit: `git add avs-memory-pgvector/ && git commit -m "feat: add pgvector memory backend"`

---

## Phase 3 Acceptance Criteria

- [ ] `LongTermMemory` trait compiles and is `Send + Sync`
- [ ] `LanceDBBackend` stores and searches entries
- [ ] `PgVectorBackend` stores and searches entries (requires running PostgreSQL)
- [ ] `truncate_messages()` and `should_trigger_summary()` work correctly
- [ ] CI includes a PostgreSQL service for pgvector tests

## Parallel Execution Notes

- `avs-memory` (trait) must be done first
- `avs-memory-lancedb` and `avs-memory-pgvector` are **independent** — can be parallelized
- Both depend on `LongTermMemory` trait from `avs-memory`

## Estimated Effort

~4-6 hours total. With parallelization: ~2-3 hours.
