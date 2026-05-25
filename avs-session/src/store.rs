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
}
