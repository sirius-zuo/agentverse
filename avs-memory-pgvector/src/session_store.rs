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

    async fn verify_ownership(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<(), SessionStoreError> {
        let id_str = session_id.to_string();
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id = $1 AND user_id = $2")
                .bind(&id_str)
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| SessionStoreError::Database(e.to_string()))?;
        if count == 0 {
            Err(SessionStoreError::NotFound(session_id))
        } else {
            Ok(())
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
                created_at   BIGINT  NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, id)")
            .execute(&self.pool)
            .await
            .map_err(|e| SessionStoreError::Database(e.to_string()))?;

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

    async fn get(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Option<Session>, SessionStoreError> {
        let id_str = session_id.to_string();
        let row = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT id, user_id, status, created_at, updated_at FROM sessions WHERE id = $1 AND user_id = $2"
        )
        .bind(&id_str).bind(user_id).fetch_optional(&self.pool).await
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
        user_id: &str,
        session_id: SessionId,
        status: SessionStatus,
    ) -> Result<(), SessionStoreError> {
        let now = chrono::Utc::now().timestamp();
        let result = sqlx::query(
            "UPDATE sessions SET status = $1, updated_at = $2 WHERE id = $3 AND user_id = $4",
        )
        .bind(status.to_string())
        .bind(now)
        .bind(session_id.to_string())
        .bind(user_id)
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
        user_id: &str,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionStoreError> {
        self.verify_ownership(user_id, session_id).await?;
        let id_str = session_id.to_string();
        let role = match message.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Tool => "tool",
        };
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO messages (session_id, role, content, created_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(&id_str)
        .bind(role)
        .bind(&message.content)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        Ok(())
    }

    async fn load_messages(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionStoreError> {
        self.verify_ownership(user_id, session_id).await?;
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT role, content FROM messages WHERE session_id = $1 ORDER BY id ASC",
        )
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SessionStoreError::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|(role, content)| Message {
                role: match role.as_str() {
                    "assistant" => MessageRole::Assistant,
                    "system" => MessageRole::System,
                    "tool" => MessageRole::Tool,
                    _ => MessageRole::User,
                },
                content,
            })
            .collect())
    }
}
