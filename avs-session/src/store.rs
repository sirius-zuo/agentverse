use crate::session::{Session, SessionId, SessionStatus};
use agentverse::memory::Message;
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum SessionMemoryError {
    #[error("database error: {0}")]
    Database(String),
    #[error("session not found: {0}")]
    NotFound(SessionId),
}

#[async_trait]
pub trait SessionMemory: Send + Sync {
    async fn create(&self, user_id: &str) -> Result<Session, SessionMemoryError>;
    async fn get(&self, session_id: SessionId) -> Result<Option<Session>, SessionMemoryError>;
    async fn update_status(
        &self,
        session_id: SessionId,
        status: SessionStatus,
    ) -> Result<(), SessionMemoryError>;
    async fn list_by_user(&self, user_id: &str) -> Result<Vec<Session>, SessionMemoryError>;
    async fn append_message(
        &self,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionMemoryError>;
    async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<(), SessionMemoryError>;
    async fn load_messages(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionMemoryError>;
    async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionMemoryError>;
    async fn advance_watermark(
        &self,
        session_id: SessionId,
        new_watermark: i64,
    ) -> Result<(), SessionMemoryError>;
    /// Returns (sequence_num, Message) tuples for all messages above the current watermark.
    async fn load_messages_above_watermark(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<(i64, Message)>, SessionMemoryError>;
    /// Deletes messages where `created_at < cutoff_ts AND sequence_num <= watermark`.
    async fn cleanup_expired_messages(
        &self,
        session_id: SessionId,
        cutoff_ts: i64,
        watermark: i64,
    ) -> Result<u64, SessionMemoryError>;
    /// Returns all active sessions across all users. Used by background workers only.
    async fn list_all_active_sessions(&self) -> Result<Vec<Session>, SessionMemoryError>;

    /// Store serialised skill context for a session (None clears it).
    /// Default implementation is a no-op so existing impls compile unchanged.
    async fn set_skill_context(
        &self,
        _session_id: SessionId,
        _context_json: Option<&str>,
    ) -> Result<(), SessionMemoryError> {
        Ok(())
    }

    /// Retrieve the serialised skill context for a session.
    /// Default returns None.
    async fn get_skill_context(
        &self,
        _session_id: SessionId,
    ) -> Result<Option<String>, SessionMemoryError> {
        Ok(None)
    }

    /// Create a session with skill context in a single operation.
    /// The default implementation is a two-step create + set; backends should
    /// override this with a single atomic INSERT to prevent orphaned sessions.
    async fn create_with_skill_context(
        &self,
        user_id: &str,
        context_json: &str,
    ) -> Result<Session, SessionMemoryError> {
        let session = self.create(user_id).await?;
        self.set_skill_context(session.id, Some(context_json)).await?;
        Ok(session)
    }
}
