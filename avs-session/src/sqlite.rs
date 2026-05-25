use crate::session::{Session, SessionId, SessionStatus};
use crate::store::{SessionStore, SessionStoreError};
use agentverse::memory::{Message, MessageRole};
use async_trait::async_trait;
use sqlx::SqlitePool;

pub struct SqliteSessionStore {
    pool: SqlitePool,
}

impl SqliteSessionStore {
    pub async fn new(database_url: &str) -> Result<Self, SessionStoreError> {
        let url = if database_url.starts_with("sqlite:") {
            database_url.to_string()
        } else {
            format!("sqlite:{}", database_url)
        };
        let url = if url == "sqlite::memory:" {
            "sqlite::memory:".to_string()
        } else if !url.contains('?') {
            format!("{}?mode=rwc", url)
        } else {
            url
        };
        let pool = SqlitePool::connect(&url)
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    fn role_to_str(role: MessageRole) -> &'static str {
        match role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Tool => "tool",
        }
    }

    fn str_to_role(role: &str) -> MessageRole {
        match role {
            "assistant" => MessageRole::Assistant,
            "system" => MessageRole::System,
            "tool" => MessageRole::Tool,
            _ => MessageRole::User,
        }
    }

    async fn migrate(&self) -> Result<(), SessionStoreError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                id          TEXT    PRIMARY KEY NOT NULL,
                user_id     TEXT    NOT NULL,
                status      TEXT    NOT NULL DEFAULT 'active',
                created_at  INTEGER NOT NULL,
                updated_at  INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id)")
            .execute(&self.pool)
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS messages (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id   TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                role         TEXT    NOT NULL,
                content      TEXT    NOT NULL,
                sequence_num INTEGER NOT NULL,
                created_at   INTEGER NOT NULL,
                UNIQUE(session_id, sequence_num)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, sequence_num)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        // Compatibility: add sequence_num to existing databases that predate this migration
        let has_sequence_num: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name = 'sequence_num'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        if has_sequence_num == 0 {
            sqlx::query("ALTER TABLE messages ADD COLUMN sequence_num INTEGER")
                .execute(&self.pool)
                .await
                .map_err(|e| SessionStoreError::Database(e.to_string()))?;

            sqlx::query(
                "UPDATE messages
                 SET sequence_num = (
                     SELECT COUNT(*)
                     FROM messages m2
                     WHERE m2.session_id = messages.session_id
                       AND m2.id <= messages.id
                 )",
            )
            .execute(&self.pool)
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        }

        let has_watermark: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'consolidation_watermark'"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        if has_watermark == 0 {
            sqlx::query(
                "ALTER TABLE sessions ADD COLUMN consolidation_watermark INTEGER NOT NULL DEFAULT 0"
            )
            .execute(&self.pool)
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        }

        Ok(())
    }
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create(&self, user_id: &str) -> Result<Session, SessionStoreError> {
        let session = Session::new(user_id);
        let id = session.id.to_string();
        let created_at = session.created_at.timestamp();
        let status = session.status.to_string();

        sqlx::query(
            "INSERT INTO sessions (id, user_id, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&id).bind(&session.user_id).bind(&status).bind(created_at).bind(created_at)
        .execute(&self.pool).await.map_err(|e| SessionStoreError::Database(e.to_string()))?;

        Ok(session)
    }

    async fn get(&self, session_id: SessionId) -> Result<Option<Session>, SessionStoreError> {
        let id_str = session_id.to_string();
        let row = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT id, user_id, status, created_at, updated_at FROM sessions WHERE id = ?",
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        row.map(
            |(id, user_id, status, created_at, updated_at)| -> Result<Session, SessionStoreError> {
                Ok(Session {
                    id: id.parse().map_err(|_| {
                        SessionStoreError::Database(format!("invalid UUID: {}", id))
                    })?,
                    user_id,
                    status: status.parse().unwrap_or(SessionStatus::Active),
                    created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default(),
                    updated_at: chrono::DateTime::from_timestamp(updated_at, 0).unwrap_or_default(),
                })
            },
        )
        .transpose()
    }

    async fn update_status(
        &self,
        session_id: SessionId,
        status: SessionStatus,
    ) -> Result<(), SessionStoreError> {
        let now = chrono::Utc::now().timestamp();
        let result = sqlx::query("UPDATE sessions SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status.to_string())
            .bind(now)
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SessionStoreError::NotFound(session_id));
        }
        Ok(())
    }

    async fn list_by_user(&self, user_id: &str) -> Result<Vec<Session>, SessionStoreError> {
        let rows = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT id, user_id, status, created_at, updated_at FROM sessions WHERE user_id = ? ORDER BY created_at DESC"
        )
        .bind(user_id).fetch_all(&self.pool).await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        rows.into_iter().map(|(id, user_id, status, created_at, updated_at)| -> Result<Session, SessionStoreError> {
            Ok(Session {
                id: id.parse().map_err(|_| SessionStoreError::Database(format!("invalid UUID: {}", id)))?,
                user_id,
                status: status.parse().unwrap_or(SessionStatus::Active),
                created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default(),
                updated_at: chrono::DateTime::from_timestamp(updated_at, 0).unwrap_or_default(),
            })
        }).collect::<Result<Vec<_>, _>>()
    }

    async fn append_message(
        &self,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionStoreError> {
        if self.get(session_id).await?.is_none() {
            return Err(SessionStoreError::NotFound(session_id));
        }

        let id_str = session_id.to_string();
        let role = Self::role_to_str(message.role);
        let now = chrono::Utc::now().timestamp();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM messages WHERE session_id = ?",
        )
        .bind(&id_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        sqlx::query(
            "INSERT INTO messages (session_id, role, content, sequence_num, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id_str)
        .bind(role)
        .bind(&message.content)
        .bind(next_sequence)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<(), SessionStoreError> {
        if self.get(session_id).await?.is_none() {
            return Err(SessionStoreError::NotFound(session_id));
        }

        let id_str = session_id.to_string();
        let now = chrono::Utc::now().timestamp();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM messages WHERE session_id = ?",
        )
        .bind(&id_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        for (offset, message) in [user_message, assistant_message].into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO messages (session_id, role, content, sequence_num, created_at)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&id_str)
            .bind(Self::role_to_str(message.role))
            .bind(&message.content)
            .bind(next_sequence + offset as i64)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn load_messages(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionStoreError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT role, content FROM messages WHERE session_id = ? ORDER BY sequence_num ASC",
        )
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|(role, content)| Message {
                role: Self::str_to_role(&role),
                content,
            })
            .collect())
    }

    async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionStoreError> {
        let wm: i64 =
            sqlx::query_scalar("SELECT consolidation_watermark FROM sessions WHERE id = ?")
                .bind(session_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        Ok(wm)
    }

    async fn advance_watermark(
        &self,
        session_id: SessionId,
        new_watermark: i64,
    ) -> Result<(), SessionStoreError> {
        let result = sqlx::query(
            "UPDATE sessions \
             SET consolidation_watermark = MAX(consolidation_watermark, ?), \
                 updated_at = ? \
             WHERE id = ?",
        )
        .bind(new_watermark)
        .bind(chrono::Utc::now().timestamp())
        .bind(session_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SessionStoreError::NotFound(session_id));
        }
        Ok(())
    }

    async fn load_messages_above_watermark(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<(i64, Message)>, SessionStoreError> {
        let wm = self.get_watermark(session_id).await?;
        let rows = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT sequence_num, role, content \
             FROM messages \
             WHERE session_id = ? AND sequence_num > ? \
             ORDER BY sequence_num ASC",
        )
        .bind(session_id.to_string())
        .bind(wm)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|(seq, role, content)| {
                (
                    seq,
                    Message {
                        role: Self::str_to_role(&role),
                        content,
                    },
                )
            })
            .collect())
    }

    async fn cleanup_expired_messages(
        &self,
        session_id: SessionId,
        cutoff_ts: i64,
        watermark: i64,
    ) -> Result<u64, SessionStoreError> {
        if watermark == 0 {
            return Ok(0);
        }
        // Cap at stored watermark to protect unconsolidated messages
        let stored_wm = self.get_watermark(session_id).await?;
        let effective_watermark = watermark.min(stored_wm);
        if effective_watermark == 0 {
            return Ok(0);
        }
        let result = sqlx::query(
            "DELETE FROM messages \
             WHERE session_id = ? AND created_at < ? AND sequence_num <= ?",
        )
        .bind(session_id.to_string())
        .bind(cutoff_ts)
        .bind(effective_watermark)
        .execute(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod watermark_tests {
    use super::*;
    use agentverse::memory::{Message, MessageRole};

    #[tokio::test]
    async fn watermark_starts_at_zero() {
        let store = SqliteSessionStore::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        let wm = store.get_watermark(session.id).await.unwrap();
        assert_eq!(wm, 0);
    }

    #[tokio::test]
    async fn advance_watermark_updates_and_is_monotonic() {
        let store = SqliteSessionStore::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        store.advance_watermark(session.id, 5).await.unwrap();
        assert_eq!(store.get_watermark(session.id).await.unwrap(), 5);
        // cannot go backward
        store.advance_watermark(session.id, 2).await.unwrap();
        assert_eq!(store.get_watermark(session.id).await.unwrap(), 5);
    }

    #[tokio::test]
    async fn load_messages_above_watermark_returns_unconsolidated() {
        let store = SqliteSessionStore::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        store
            .append_turn(
                session.id,
                Message {
                    role: MessageRole::User,
                    content: "q".to_string(),
                },
                Message {
                    role: MessageRole::Assistant,
                    content: "a".to_string(),
                },
            )
            .await
            .unwrap();
        // watermark=0: both messages (seq 1, 2) are above watermark
        let msgs = store
            .load_messages_above_watermark(session.id)
            .await
            .unwrap();
        assert_eq!(msgs.len(), 2);
        // advance to 1: only seq 2 remains unconsolidated
        store.advance_watermark(session.id, 1).await.unwrap();
        let msgs = store
            .load_messages_above_watermark(session.id)
            .await
            .unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1.content, "a");
    }

    #[tokio::test]
    async fn cleanup_respects_watermark_safety_invariant() {
        let store = SqliteSessionStore::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        store
            .append_turn(
                session.id,
                Message {
                    role: MessageRole::User,
                    content: "old".to_string(),
                },
                Message {
                    role: MessageRole::Assistant,
                    content: "old reply".to_string(),
                },
            )
            .await
            .unwrap();
        let far_future = chrono::Utc::now().timestamp() + 100_000;
        // watermark=0: cleanup deletes nothing regardless of cutoff
        let deleted = store
            .cleanup_expired_messages(session.id, far_future, 0)
            .await
            .unwrap();
        assert_eq!(deleted, 0);
        // advance watermark to cover both messages, then cleanup purges them
        store.advance_watermark(session.id, 2).await.unwrap();
        let deleted = store
            .cleanup_expired_messages(session.id, far_future, 2)
            .await
            .unwrap();
        assert_eq!(deleted, 2);
        let all = store.load_messages(session.id).await.unwrap();
        assert!(all.is_empty());
    }
}
