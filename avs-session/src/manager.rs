use crate::session::{Session, SessionId, SessionStatus};
use crate::store::{SessionStore, SessionStoreError};
use agentverse::memory::Message;
use std::sync::Arc;

pub struct SessionManager {
    store: Arc<dyn SessionStore>,
}

impl SessionManager {
    pub fn new(store: Arc<dyn SessionStore>) -> Self {
        Self { store }
    }

    pub async fn create_session(&self, user_id: &str) -> Result<SessionId, SessionStoreError> {
        let session = self.store.create(user_id).await?;
        Ok(session.id)
    }

    pub async fn get_session(
        &self,
        session_id: SessionId,
    ) -> Result<Option<Session>, SessionStoreError> {
        self.store.get(session_id).await
    }

    pub async fn assert_owner(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<(), SessionStoreError> {
        match self.store.get(session_id).await? {
            Some(session) if session.user_id == user_id => Ok(()),
            _ => Err(SessionStoreError::NotFound(session_id)),
        }
    }

    pub async fn end_session(&self, session_id: SessionId) -> Result<(), SessionStoreError> {
        self.store
            .update_status(session_id, SessionStatus::Completed)
            .await
    }

    pub async fn list_sessions(&self, user_id: &str) -> Result<Vec<Session>, SessionStoreError> {
        self.store.list_by_user(user_id).await
    }

    pub async fn load_messages(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionStoreError> {
        self.store.load_messages(session_id).await
    }

    pub async fn append_message(
        &self,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionStoreError> {
        self.store.append_message(session_id, message).await
    }

    pub async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<(), SessionStoreError> {
        self.store
            .append_turn(session_id, user_message, assistant_message)
            .await
    }
}
