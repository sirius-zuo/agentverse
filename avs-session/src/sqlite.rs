use crate::session::{Session, SessionId, SessionStatus};
use crate::store::{SessionMemory, SessionMemoryError};
use agentverse::memory::{Message, MessageRole};
use async_trait::async_trait;
use sqlx::SqlitePool;

pub struct SqliteSessionMemory {
    pool: SqlitePool,
}

impl SqliteSessionMemory {
    pub async fn new(database_url: &str) -> Result<Self, SessionMemoryError> {
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

    fn str_to_role(role: &str) -> Result<MessageRole, SessionMemoryError> {
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
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| SessionMemoryError::Database(e.to_string()))
    }
}

#[async_trait]
impl SessionMemory for SqliteSessionMemory {
    async fn create(&self, user_id: &str) -> Result<Session, SessionMemoryError> {
        let session = Session::new(user_id);
        let id = session.id.to_string();
        let created_at = session.created_at.timestamp();
        let status = session.status.to_string();

        sqlx::query(
            "INSERT INTO sessions (id, user_id, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&id).bind(&session.user_id).bind(&status).bind(created_at).bind(created_at)
        .execute(&self.pool).await.map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        Ok(session)
    }

    async fn get(&self, session_id: SessionId) -> Result<Option<Session>, SessionMemoryError> {
        let id_str = session_id.to_string();
        let row = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT id, user_id, status, created_at, updated_at FROM sessions WHERE id = ?",
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
        let result = sqlx::query("UPDATE sessions SET status = ?, updated_at = ? WHERE id = ?")
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
            "SELECT id, user_id, status, created_at, updated_at FROM sessions WHERE user_id = ? ORDER BY created_at DESC"
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
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM messages WHERE session_id = ?",
        )
        .bind(&id_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

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
            "SELECT COALESCE(MAX(sequence_num), 0) + 1 FROM messages WHERE session_id = ?",
        )
        .bind(&id_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

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
            "SELECT role, content FROM messages WHERE session_id = ? ORDER BY sequence_num ASC",
        )
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        rows.into_iter()
            .map(|(role, content)| {
                Ok(Message {
                    role: Self::str_to_role(&role)?,
                    content,
                })
            })
            .collect::<Result<Vec<_>, SessionMemoryError>>()
    }

    async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionMemoryError> {
        let wm: i64 =
            sqlx::query_scalar("SELECT consolidation_watermark FROM sessions WHERE id = ?")
                .bind(session_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        Ok(wm)
    }

    async fn advance_watermark(
        &self,
        session_id: SessionId,
        new_watermark: i64,
    ) -> Result<(), SessionMemoryError> {
        let result = sqlx::query(
            "UPDATE sessions \
             SET consolidation_watermark = MAX(consolidation_watermark, ?) \
             WHERE id = ?",
        )
        .bind(new_watermark)
        .bind(session_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        Ok(())
    }

    async fn load_messages_above_watermark(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<(i64, Message)>, SessionMemoryError> {
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
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        rows.into_iter()
            .map(|(seq, role, content)| {
                Ok((
                    seq,
                    Message {
                        role: Self::str_to_role(&role)?,
                        content,
                    },
                ))
            })
            .collect::<Result<Vec<_>, SessionMemoryError>>()
    }

    async fn list_all_active_sessions(&self) -> Result<Vec<Session>, SessionMemoryError> {
        let rows = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT id, user_id, status, created_at, updated_at \
             FROM sessions WHERE status = 'active' ORDER BY updated_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

        rows.into_iter()
            .map(|(id, user_id, status, created_at, updated_at)| {
                Ok(Session {
                    id: id.parse().map_err(|_| {
                        SessionMemoryError::Database(format!("invalid UUID: {}", id))
                    })?,
                    user_id,
                    status: status.parse().unwrap_or(SessionStatus::Active),
                    created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default(),
                    updated_at: chrono::DateTime::from_timestamp(updated_at, 0).unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, _>>()
    }

    async fn cleanup_expired_messages(
        &self,
        session_id: SessionId,
        cutoff_ts: i64,
        watermark: i64,
    ) -> Result<u64, SessionMemoryError> {
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
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
        Ok(result.rows_affected())
    }

    async fn set_skill_context(
        &self,
        session_id: SessionId,
        context_json: Option<&str>,
    ) -> Result<(), SessionMemoryError> {
        let result = sqlx::query("UPDATE sessions SET skill_context_json = ? WHERE id = ?")
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
            sqlx::query_scalar("SELECT skill_context_json FROM sessions WHERE id = ?")
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
        let result = sqlx::query("UPDATE sessions SET phase_opening_context = ? WHERE id = ?")
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
            sqlx::query_scalar("SELECT phase_opening_context FROM sessions WHERE id = ?")
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
             SET skill_context_json = ?, phase_opening_context = ? \
             WHERE id = ?",
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
             VALUES (?, ?, ?, ?, ?, ?)",
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
        let result = sqlx::query("UPDATE sessions SET interrupted_state = ? WHERE id = ?")
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
            sqlx::query_scalar("SELECT interrupted_state FROM sessions WHERE id = ?")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_to_role_rejects_unknown_role() {
        let err = SqliteSessionMemory::str_to_role("wizard").unwrap_err();
        assert!(err.to_string().contains("wizard"));
    }

    #[test]
    fn str_to_role_maps_known_roles() {
        assert!(matches!(
            SqliteSessionMemory::str_to_role("user"),
            Ok(MessageRole::User)
        ));
        assert!(matches!(
            SqliteSessionMemory::str_to_role("assistant"),
            Ok(MessageRole::Assistant)
        ));
        assert!(matches!(
            SqliteSessionMemory::str_to_role("system"),
            Ok(MessageRole::System)
        ));
        assert!(matches!(
            SqliteSessionMemory::str_to_role("tool"),
            Ok(MessageRole::Tool)
        ));
    }
}

#[cfg(test)]
mod skill_context_tests {
    use super::*;

    #[tokio::test]
    async fn create_with_skill_context_stores_context_atomically() {
        let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
        let ctx_json =
            r#"{"instructions":"atomic","documents":[],"tools":[],"max_iterations":null}"#;

        let session = store
            .create_with_skill_context("alice", ctx_json)
            .await
            .unwrap();

        // Context must be set — no second call needed
        let retrieved = store.get_skill_context(session.id).await.unwrap();
        assert!(retrieved.is_some());
        assert!(retrieved.unwrap().contains("atomic"));
    }

    #[tokio::test]
    async fn skill_context_roundtrips() {
        let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();

        // Initially None
        let ctx = store.get_skill_context(session.id).await.unwrap();
        assert!(ctx.is_none());

        // Set some JSON
        store
            .set_skill_context(
                session.id,
                Some(r#"{"instructions":"test","documents":[],"tools":[],"max_iterations":null}"#),
            )
            .await
            .unwrap();

        let ctx = store.get_skill_context(session.id).await.unwrap();
        assert!(ctx.is_some());
        assert!(ctx.unwrap().contains("instructions"));

        // Clear it
        store.set_skill_context(session.id, None).await.unwrap();
        let ctx = store.get_skill_context(session.id).await.unwrap();
        assert!(ctx.is_none());
    }

    #[tokio::test]
    async fn set_and_get_phase_opening_context() {
        let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
        let session = store.create("bob").await.unwrap();

        // Initially None
        let ctx = store.get_phase_opening_context(session.id).await.unwrap();
        assert!(ctx.is_none());

        // Set it
        store
            .set_phase_opening_context(
                session.id,
                Some("Summary of previous phase: Found 3 entities."),
            )
            .await
            .unwrap();

        // Retrieve it
        let ctx = store.get_phase_opening_context(session.id).await.unwrap();
        assert_eq!(ctx.unwrap(), "Summary of previous phase: Found 3 entities.");

        // Clear it
        store
            .set_phase_opening_context(session.id, None)
            .await
            .unwrap();
        let ctx = store.get_phase_opening_context(session.id).await.unwrap();
        assert!(ctx.is_none());
    }

    #[tokio::test]
    async fn get_phase_opening_context_errors_on_missing_session() {
        let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
        let fake_id = uuid::Uuid::new_v4();
        let result = store.get_phase_opening_context(fake_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn apply_phase_transition_sets_both_fields() {
        let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();

        store
            .apply_phase_transition(
                session.id,
                r#"{"instructions":"new skill","tools":[],"strategy":null}"#,
                "Context from previous phase: Found 5 items.",
            )
            .await
            .unwrap();

        let skill_ctx = store.get_skill_context(session.id).await.unwrap();
        assert!(
            skill_ctx.unwrap().contains("new skill"),
            "skill_context_json must be updated"
        );

        let phase_ctx = store.get_phase_opening_context(session.id).await.unwrap();
        assert_eq!(
            phase_ctx.unwrap(),
            "Context from previous phase: Found 5 items.",
            "phase_opening_context must be updated"
        );
    }
}

#[cfg(test)]
mod watermark_tests {
    use super::*;
    use agentverse::memory::{Message, MessageRole};

    #[tokio::test]
    async fn watermark_starts_at_zero() {
        let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        let wm = store.get_watermark(session.id).await.unwrap();
        assert_eq!(wm, 0);
    }

    #[tokio::test]
    async fn advance_watermark_updates_and_is_monotonic() {
        let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        store.advance_watermark(session.id, 5).await.unwrap();
        assert_eq!(store.get_watermark(session.id).await.unwrap(), 5);
        // cannot go backward
        store.advance_watermark(session.id, 2).await.unwrap();
        assert_eq!(store.get_watermark(session.id).await.unwrap(), 5);
    }

    #[tokio::test]
    async fn load_messages_above_watermark_returns_unconsolidated() {
        let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
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
        let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
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

#[cfg(test)]
mod interrupted_state_tests {
    use super::*;
    use crate::store::InterruptedState;

    #[tokio::test]
    async fn set_and_get_interrupted_state() {
        let mem = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
        let session = mem.create("alice").await.unwrap();

        let state = InterruptedState::PendingToolCall {
            approval_id: uuid::Uuid::new_v4().to_string(),
            kind_json: "{}".to_string(),
            history_json: "[]".to_string(),
            pending_calls_json: "[]".to_string(),
            active_tool_names: vec!["file_read".to_string()],
            skill_context_json: None,
        };
        let json = serde_json::to_string(&state).unwrap();

        mem.set_interrupted_state(session.id, Some(&json))
            .await
            .unwrap();
        let loaded = mem.get_interrupted_state(session.id).await.unwrap();
        assert!(loaded.is_some());
        let loaded_state: InterruptedState = serde_json::from_str(&loaded.unwrap()).unwrap();
        assert_eq!(loaded_state.approval_id_str(), state.approval_id_str());
    }

    #[tokio::test]
    async fn clear_interrupted_state() {
        let mem = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
        let session = mem.create("alice").await.unwrap();
        let state_json = serde_json::json!({"approval_id": "x"}).to_string();
        mem.set_interrupted_state(session.id, Some(&state_json))
            .await
            .unwrap();
        mem.set_interrupted_state(session.id, None).await.unwrap();
        let loaded = mem.get_interrupted_state(session.id).await.unwrap();
        assert!(loaded.is_none());
    }
}
