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
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Option<Session>, SessionStoreError> {
        self.store.get(user_id, session_id).await
    }

    pub async fn end_session(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<(), SessionStoreError> {
        self.store
            .update_status(user_id, session_id, SessionStatus::Completed)
            .await
    }

    pub async fn list_sessions(&self, user_id: &str) -> Result<Vec<Session>, SessionStoreError> {
        self.store.list_by_user(user_id).await
    }

    pub async fn load_messages(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionStoreError> {
        self.store.load_messages(user_id, session_id).await
    }

    pub async fn append_message(
        &self,
        user_id: &str,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionStoreError> {
        self.store
            .append_message(user_id, session_id, message)
            .await
    }
}
