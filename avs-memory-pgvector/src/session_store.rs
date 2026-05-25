use agentverse::memory::{Message, MessageRole};
use agentverse_session::session::{Session, SessionId, SessionStatus};
use agentverse_session::store::{SessionStore, SessionStoreError};
use async_trait::async_trait;
use sqlx::PgPool;

pub struct PostgresSessionStore {
    pool: PgPool,
}

impl PostgresSessionStore {
    pub async fn new(database_url: &str) -> Result<Self, SessionStoreError> {
        let pool = PgPool::connect(database_url)
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
                id          TEXT    PRIMARY KEY,
                user_id     TEXT    NOT NULL,
                status      TEXT    NOT NULL DEFAULT 'active',
                created_at  BIGINT  NOT NULL,
                updated_at  BIGINT  NOT NULL
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
                id           BIGSERIAL PRIMARY KEY,
                session_id   TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                role         TEXT    NOT NULL,
                content      TEXT    NOT NULL,
                sequence_num BIGINT  NOT NULL,
                created_at   BIGINT  NOT NULL,
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
            "SELECT COUNT(*)
             FROM information_schema.columns
             WHERE table_name = 'messages' AND column_name = 'sequence_num'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        if has_sequence_num == 0 {
            sqlx::query("ALTER TABLE messages ADD COLUMN sequence_num BIGINT")
                .execute(&self.pool)
                .await
                .map_err(|e| SessionStoreError::Database(e.to_string()))?;

            sqlx::query(
                "UPDATE messages
                 SET sequence_num = ranked.seq
                 FROM (
                     SELECT id, ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY id) AS seq
                     FROM messages
                 ) ranked
                 WHERE messages.id = ranked.id",
            )
            .execute(&self.pool)
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        }

        let has_watermark: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM information_schema.columns
             WHERE table_name = 'sessions' AND column_name = 'consolidation_watermark'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        if has_watermark == 0 {
            sqlx::query(
                "ALTER TABLE sessions ADD COLUMN consolidation_watermark BIGINT NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        }

        Ok(())
    }
}

#[async_trait]
impl SessionStore for PostgresSessionStore {
    async fn create(&self, user_id: &str) -> Result<Session, SessionStoreError> {
        let session = Session::new(user_id);
        let id = session.id.to_string();
        let created_at = session.created_at.timestamp();
        let status = session.status.to_string();

        sqlx::query(
            "INSERT INTO sessions (id, user_id, status, created_at, updated_at) VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(&id).bind(&session.user_id).bind(&status).bind(created_at).bind(created_at)
        .execute(&self.pool).await.map_err(|e| SessionStoreError::Database(e.to_string()))?;

        Ok(session)
    }

    async fn get(&self, session_id: SessionId) -> Result<Option<Session>, SessionStoreError> {
        let id_str = session_id.to_string();
        let row = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT id, user_id, status, created_at, updated_at FROM sessions WHERE id = $1",
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
        let result = sqlx::query("UPDATE sessions SET status = $1, updated_at = $2 WHERE id = $3")
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
            "SELECT id, user_id, status, created_at, updated_at FROM sessions WHERE user_id = $1 ORDER BY created_at DESC"
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
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM messages WHERE session_id = $1",
        )
        .bind(&id_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        sqlx::query(
            "INSERT INTO messages (session_id, role, content, sequence_num, created_at)
             VALUES ($1, $2, $3, $4, $5)",
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
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM messages WHERE session_id = $1",
        )
        .bind(&id_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        for (offset, message) in [user_message, assistant_message].into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO messages (session_id, role, content, sequence_num, created_at)
                 VALUES ($1, $2, $3, $4, $5)",
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
            "SELECT role, content FROM messages WHERE session_id = $1 ORDER BY sequence_num ASC",
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
            sqlx::query_scalar("SELECT consolidation_watermark FROM sessions WHERE id = $1")
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
        sqlx::query(
            "UPDATE sessions \
             SET consolidation_watermark = GREATEST(consolidation_watermark, $1), \
                 updated_at = $2 \
             WHERE id = $3",
        )
        .bind(new_watermark)
        .bind(chrono::Utc::now().timestamp())
        .bind(session_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;
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
             WHERE session_id = $1 AND sequence_num > $2 \
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
        let result = sqlx::query(
            "DELETE FROM messages \
             WHERE session_id = $1 AND created_at < $2 AND sequence_num <= $3",
        )
        .bind(session_id.to_string())
        .bind(cutoff_ts)
        .bind(watermark)
        .execute(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        Ok(result.rows_affected())
    }
}
