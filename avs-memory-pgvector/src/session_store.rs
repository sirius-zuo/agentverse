use agentverse::memory::{Message, MessageRole};
use agentverse_memory::session::{
    Session, SessionId, SessionMemory, SessionMemoryError, SessionStatus,
};
use async_trait::async_trait;
use sqlx::PgPool;

pub struct PostgresSessionMemory {
    pub(crate) pool: PgPool,
}

impl PostgresSessionMemory {
    pub async fn new(database_url: &str) -> Result<Self, SessionMemoryError> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
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

    pub(crate) fn str_to_role(role: &str) -> Result<MessageRole, SessionMemoryError> {
        match role {
            "user" => Ok(MessageRole::User),
            "assistant" => Ok(MessageRole::Assistant),
            "system" => Ok(MessageRole::System),
            "tool" => Ok(MessageRole::Tool),
            other => Err(SessionMemoryError::Database(format!(
                "corrupt message row: unknown role '{other}'"
            ))),
        }
    }

    async fn migrate(&self) -> Result<(), SessionMemoryError> {
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
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id)")
            .execute(&self.pool)
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

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
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, sequence_num)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        // Compatibility: add sequence_num to existing databases that predate this migration
        let has_sequence_num: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM information_schema.columns
             WHERE table_name = 'messages' AND column_name = 'sequence_num'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        if has_sequence_num == 0 {
            sqlx::query("ALTER TABLE messages ADD COLUMN sequence_num BIGINT")
                .execute(&self.pool)
                .await
                .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

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
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        }

        let has_watermark: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM information_schema.columns
             WHERE table_name = 'sessions' AND column_name = 'consolidation_watermark'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        if has_watermark == 0 {
            sqlx::query(
                "ALTER TABLE sessions ADD COLUMN consolidation_watermark BIGINT NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        }

        // Skill context column (added by skill-system plan)
        let has_skill_ctx: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM information_schema.columns
             WHERE table_name = 'sessions' AND column_name = 'skill_context_json'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        if has_skill_ctx == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN skill_context_json TEXT")
                .execute(&self.pool)
                .await
                .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        }

        // Phase opening context column (single-agent multi-phase model)
        let has_phase_ctx: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM information_schema.columns
             WHERE table_name = 'sessions' AND column_name = 'phase_opening_context'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        if has_phase_ctx == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN phase_opening_context TEXT")
                .execute(&self.pool)
                .await
                .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        }

        // Interrupted state column (HITL suspend/resume)
        let has_interrupted_state: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM information_schema.columns
             WHERE table_name = 'sessions' AND column_name = 'interrupted_state'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        if has_interrupted_state == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN interrupted_state TEXT")
                .execute(&self.pool)
                .await
                .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        }

        Ok(())
    }
}

/// Encode a message's content blocks for storage in the `messages.content`
/// TEXT column. `ContentBlock`'s `Serialize` impl is total (no error path
/// that can occur for real content), so this cannot fail in practice.
fn encode_content(content: &[agentverse::memory::ContentBlock]) -> String {
    serde_json::to_string(content).expect("ContentBlock serialization is infallible")
}

/// Decode a `messages.content` column value back into `Vec<ContentBlock>`.
///
/// Rows written *before* this refactor hold a plain string (e.g. `"hello"`),
/// not JSON — `serde_json::from_str::<Vec<ContentBlock>>` fails on those,
/// since a bare string is not valid JSON for a sequence type. Rather than
/// hard-failing every pre-existing session's first load after this deploy,
/// fall back to wrapping the raw text in a single `Text` block: this is
/// lossless (a legacy row only ever held plain text) and consistent with
/// this project's "no migration for persisted session data" decision.
pub(crate) fn decode_content(raw: &str) -> Vec<agentverse::memory::ContentBlock> {
    serde_json::from_str(raw).unwrap_or_else(|_| {
        vec![agentverse::memory::ContentBlock::Text {
            text: raw.to_string(),
        }]
    })
}

#[async_trait]
impl SessionMemory for PostgresSessionMemory {
    async fn create(&self, user_id: &str) -> Result<Session, SessionMemoryError> {
        let session = Session::new(user_id);
        let id = session.id.to_string();
        let created_at = session.created_at.timestamp();
        let status = session.status.to_string();

        sqlx::query(
            "INSERT INTO sessions (id, user_id, status, created_at, updated_at) VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(&id).bind(&session.user_id).bind(&status).bind(created_at).bind(created_at)
        .execute(&self.pool).await.map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        Ok(session)
    }

    async fn get(&self, session_id: SessionId) -> Result<Option<Session>, SessionMemoryError> {
        let id_str = session_id.to_string();
        let row = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT id, user_id, status, created_at, updated_at FROM sessions WHERE id = $1",
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        row.map(
            |(id, user_id, status, created_at, updated_at)| -> Result<Session, SessionMemoryError> {
                Ok(Session {
                    id: id.parse().map_err(|_| {
                        SessionMemoryError::Database(format!("invalid UUID: {}", id))
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
    ) -> Result<(), SessionMemoryError> {
        let now = chrono::Utc::now().timestamp();
        let result = sqlx::query("UPDATE sessions SET status = $1, updated_at = $2 WHERE id = $3")
            .bind(status.to_string())
            .bind(now)
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        Ok(())
    }

    async fn list_by_user(&self, user_id: &str) -> Result<Vec<Session>, SessionMemoryError> {
        let rows = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT id, user_id, status, created_at, updated_at FROM sessions WHERE user_id = $1 ORDER BY created_at DESC"
        )
        .bind(user_id).fetch_all(&self.pool).await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        rows.into_iter().map(|(id, user_id, status, created_at, updated_at)| -> Result<Session, SessionMemoryError> {
            Ok(Session {
                id: id.parse().map_err(|_| SessionMemoryError::Database(format!("invalid UUID: {}", id)))?,
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
    ) -> Result<(), SessionMemoryError> {
        if self.get(session_id).await?.is_none() {
            return Err(SessionMemoryError::NotFound(session_id));
        }

        let id_str = session_id.to_string();
        let role = Self::role_to_str(message.role);
        let now = chrono::Utc::now().timestamp();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM messages WHERE session_id = $1",
        )
        .bind(&id_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        sqlx::query(
            "INSERT INTO messages (session_id, role, content, sequence_num, created_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&id_str)
        .bind(role)
        .bind(encode_content(&message.content))
        .bind(next_sequence)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        Ok(())
    }

    async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<(), SessionMemoryError> {
        if self.get(session_id).await?.is_none() {
            return Err(SessionMemoryError::NotFound(session_id));
        }

        let id_str = session_id.to_string();
        let now = chrono::Utc::now().timestamp();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM messages WHERE session_id = $1",
        )
        .bind(&id_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        for (offset, message) in [user_message, assistant_message].into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO messages (session_id, role, content, sequence_num, created_at)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(&id_str)
            .bind(Self::role_to_str(message.role))
            .bind(encode_content(&message.content))
            .bind(next_sequence + offset as i64)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        Ok(())
    }

    async fn load_messages(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionMemoryError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT role, content FROM messages WHERE session_id = $1 ORDER BY sequence_num ASC",
        )
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        rows.into_iter()
            .map(|(role, content)| {
                Ok(Message {
                    role: Self::str_to_role(&role)?,
                    content: decode_content(&content),
                })
            })
            .collect::<Result<Vec<_>, SessionMemoryError>>()
    }

    async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionMemoryError> {
        crate::session_store_maintenance::get_watermark(self, session_id).await
    }

    async fn advance_watermark(
        &self,
        session_id: SessionId,
        new_watermark: i64,
    ) -> Result<(), SessionMemoryError> {
        crate::session_store_maintenance::advance_watermark(self, session_id, new_watermark).await
    }

    async fn load_messages_above_watermark(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<(i64, Message)>, SessionMemoryError> {
        crate::session_store_maintenance::load_messages_above_watermark(self, session_id).await
    }

    async fn cleanup_expired_messages(
        &self,
        session_id: SessionId,
        cutoff_ts: i64,
        watermark: i64,
    ) -> Result<u64, SessionMemoryError> {
        crate::session_store_maintenance::cleanup_expired_messages(
            self, session_id, cutoff_ts, watermark,
        )
        .await
    }

    async fn delete_ended_sessions_before(
        &self,
        cutoff_ts: i64,
    ) -> Result<u64, SessionMemoryError> {
        crate::session_store_maintenance::delete_ended_sessions_before(self, cutoff_ts).await
    }

    async fn delete_session(&self, session_id: SessionId) -> Result<(), SessionMemoryError> {
        crate::session_store_maintenance::delete_session(self, session_id).await
    }

    async fn list_sessions_needing_maintenance(&self) -> Result<Vec<Session>, SessionMemoryError> {
        crate::session_store_maintenance::list_sessions_needing_maintenance(self).await
    }

    async fn set_skill_context(
        &self,
        session_id: SessionId,
        context_json: Option<&str>,
    ) -> Result<(), SessionMemoryError> {
        let result = sqlx::query("UPDATE sessions SET skill_context_json = $1 WHERE id = $2")
            .bind(context_json)
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        Ok(())
    }

    async fn get_skill_context(
        &self,
        session_id: SessionId,
    ) -> Result<Option<String>, SessionMemoryError> {
        let row: Option<Option<String>> =
            sqlx::query_scalar("SELECT skill_context_json FROM sessions WHERE id = $1")
                .bind(session_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        match row {
            None => Err(SessionMemoryError::NotFound(session_id)),
            Some(v) => Ok(v),
        }
    }

    async fn set_phase_opening_context(
        &self,
        session_id: SessionId,
        context: Option<&str>,
    ) -> Result<(), SessionMemoryError> {
        let result = sqlx::query("UPDATE sessions SET phase_opening_context = $1 WHERE id = $2")
            .bind(context)
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        Ok(())
    }

    async fn get_phase_opening_context(
        &self,
        session_id: SessionId,
    ) -> Result<Option<String>, SessionMemoryError> {
        let row: Option<Option<String>> =
            sqlx::query_scalar("SELECT phase_opening_context FROM sessions WHERE id = $1")
                .bind(session_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        match row {
            None => Err(SessionMemoryError::NotFound(session_id)),
            Some(v) => Ok(v),
        }
    }

    async fn apply_phase_transition(
        &self,
        session_id: SessionId,
        skill_ctx_json: &str,
        phase_ctx: &str,
    ) -> Result<(), SessionMemoryError> {
        let result = sqlx::query(
            "UPDATE sessions \
             SET skill_context_json = $1, phase_opening_context = $2 \
             WHERE id = $3",
        )
        .bind(skill_ctx_json)
        .bind(phase_ctx)
        .bind(session_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        Ok(())
    }

    async fn create_with_skill_context(
        &self,
        user_id: &str,
        context_json: &str,
    ) -> Result<Session, SessionMemoryError> {
        let session = Session::new(user_id);
        let id = session.id.to_string();
        let created_at = session.created_at.timestamp();
        let status = session.status.to_string();

        sqlx::query(
            "INSERT INTO sessions (id, user_id, status, created_at, updated_at, skill_context_json)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&id)
        .bind(&session.user_id)
        .bind(&status)
        .bind(created_at)
        .bind(created_at)
        .bind(context_json)
        .execute(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        Ok(session)
    }

    async fn set_interrupted_state(
        &self,
        session_id: SessionId,
        state_json: Option<&str>,
    ) -> Result<(), SessionMemoryError> {
        let result = sqlx::query("UPDATE sessions SET interrupted_state = $1 WHERE id = $2")
            .bind(state_json)
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        Ok(())
    }

    async fn get_interrupted_state(
        &self,
        session_id: SessionId,
    ) -> Result<Option<String>, SessionMemoryError> {
        let row: Option<Option<String>> =
            sqlx::query_scalar("SELECT interrupted_state FROM sessions WHERE id = $1")
                .bind(session_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        match row {
            None => Err(SessionMemoryError::NotFound(session_id)),
            Some(v) => Ok(v),
        }
    }
}
