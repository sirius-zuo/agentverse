use crate::session::{Session, SessionId, SessionStatus};
use agentverse::memory::Message;
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    #[error("database error: {0}")]
    Database(String),
    #[error("session not found: {0}")]
    NotFound(SessionId),
}

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn create(&self, user_id: &str) -> Result<Session, SessionStoreError>;
    async fn get(&self, session_id: SessionId) -> Result<Option<Session>, SessionStoreError>;
    async fn update_status(
        &self,
        session_id: SessionId,
        status: SessionStatus,
    ) -> Result<(), SessionStoreError>;
    async fn list_by_user(&self, user_id: &str) -> Result<Vec<Session>, SessionStoreError>;
    async fn append_message(
        &self,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionStoreError>;
    async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<(), SessionStoreError>;
    async fn load_messages(&self, session_id: SessionId)
        -> Result<Vec<Message>, SessionStoreError>;
    async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionStoreError>;
    async fn advance_watermark(
        &self,
        session_id: SessionId,
        new_watermark: i64,
    ) -> Result<(), SessionStoreError>;
    /// Returns (sequence_num, Message) tuples for all messages above the current watermark.
    async fn load_messages_above_watermark(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<(i64, Message)>, SessionStoreError>;
    /// Deletes messages where `created_at < cutoff_ts AND sequence_num <= watermark`.
    async fn cleanup_expired_messages(
        &self,
        session_id: SessionId,
        cutoff_ts: i64,
        watermark: i64,
    ) -> Result<u64, SessionStoreError>;
    /// Returns all active sessions across all users. Used by background workers only.
    async fn list_all_active_sessions(&self) -> Result<Vec<Session>, SessionStoreError>;
}
