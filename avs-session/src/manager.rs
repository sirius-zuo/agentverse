use crate::session::{Session, SessionId, SessionStatus};
use crate::store::{SessionMemory, SessionMemoryError};
use agentverse::memory::Message;
use std::sync::Arc;

pub struct SessionManager {
    store: Arc<dyn SessionMemory>,
}

impl SessionManager {
    pub fn new(store: Arc<dyn SessionMemory>) -> Self {
        Self { store }
    }

    pub async fn create_session(&self, user_id: &str) -> Result<SessionId, SessionMemoryError> {
        let session = self.store.create(user_id).await?;
        Ok(session.id)
    }

    pub async fn get_session(
        &self,
        session_id: SessionId,
    ) -> Result<Option<Session>, SessionMemoryError> {
        self.store.get(session_id).await
    }

    pub async fn assert_owner(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<(), SessionMemoryError> {
        match self.store.get(session_id).await? {
            Some(session) if session.user_id == user_id => Ok(()),
            _ => Err(SessionMemoryError::NotFound(session_id)),
        }
    }

    pub async fn end_session(&self, session_id: SessionId) -> Result<(), SessionMemoryError> {
        self.store
            .update_status(session_id, SessionStatus::Completed)
            .await
    }

    pub async fn list_sessions(&self, user_id: &str) -> Result<Vec<Session>, SessionMemoryError> {
        self.store.list_by_user(user_id).await
    }

    pub async fn load_messages(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionMemoryError> {
        self.store.load_messages(session_id).await
    }

    pub async fn append_message(
        &self,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionMemoryError> {
        self.store.append_message(session_id, message).await
    }

    pub async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<(), SessionMemoryError> {
        self.store
            .append_turn(session_id, user_message, assistant_message)
            .await
    }
}
