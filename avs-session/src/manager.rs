use std::sync::Arc;
use agentverse::memory::Message;
use crate::session::{Session, SessionId, SessionStatus};
use crate::store::{SessionStore, SessionStoreError};

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

    pub async fn get_session(&self, session_id: SessionId) -> Result<Option<Session>, SessionStoreError> {
        self.store.get(session_id).await
    }

    pub async fn end_session(&self, session_id: SessionId) -> Result<(), SessionStoreError> {
        self.store.update_status(session_id, SessionStatus::Completed).await
    }

    pub async fn list_sessions(&self, user_id: &str) -> Result<Vec<Session>, SessionStoreError> {
        self.store.list_by_user(user_id).await
    }

    pub async fn load_messages(&self, session_id: SessionId) -> Result<Vec<Message>, SessionStoreError> {
        self.store.load_messages(session_id).await
    }

    pub async fn append_message(&self, session_id: SessionId, message: Message) -> Result<(), SessionStoreError> {
        self.store.append_message(session_id, message).await
    }
}
