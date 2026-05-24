use std::sync::Arc;

use agentverse::LlmRunner;
use agentverse::memory::{Message, MessageRole};

use crate::manager::SessionManager;
use crate::session::{Session, SessionId};
use crate::store::{SessionStore, SessionStoreError};

#[derive(Debug, thiserror::Error)]
pub enum SessionAgentError {
    #[error("session error: {0}")]
    Session(#[from] SessionStoreError),
    #[error("llm error: {0}")]
    Llm(#[from] agentverse::AgentError),
}

pub struct Agent {
    runner: Arc<LlmRunner>,
    sessions: SessionManager,
}

impl Agent {
    pub fn new(runner: Arc<LlmRunner>, store: Arc<dyn SessionStore>) -> Self {
        Self {
            runner,
            sessions: SessionManager::new(store),
        }
    }

    pub async fn create_session(&self, user_id: &str) -> Result<SessionId, SessionAgentError> {
        Ok(self.sessions.create_session(user_id).await?)
    }

    pub async fn get_session(&self, user_id: &str, session_id: SessionId) -> Result<Option<Session>, SessionAgentError> {
        Ok(self.sessions.get_session(user_id, session_id).await?)
    }

    pub async fn end_session(&self, user_id: &str, session_id: SessionId) -> Result<(), SessionAgentError> {
        Ok(self.sessions.end_session(user_id, session_id).await?)
    }

    pub async fn list_sessions(&self, user_id: &str) -> Result<Vec<Session>, SessionAgentError> {
        Ok(self.sessions.list_sessions(user_id).await?)
    }

    pub async fn load_messages(&self, user_id: &str, session_id: SessionId) -> Result<Vec<Message>, SessionAgentError> {
        Ok(self.sessions.load_messages(user_id, session_id).await?)
    }

    pub async fn invoke(&self, user_id: &str, session_id: SessionId, input: &str) -> Result<String, SessionAgentError> {
        // Load existing history
        let mut messages = self.sessions.load_messages(user_id, session_id).await?;

        // Build in-memory user message (do not persist yet)
        let user_msg = Message { role: MessageRole::User, content: input.to_string() };
        messages.push(user_msg.clone());

        // Call LLM — if this fails, nothing has been persisted, caller can safely retry
        let response = self.runner.invoke(messages).await?;

        // Persist both messages only after a successful LLM response
        self.sessions.append_message(user_id, session_id, user_msg).await?;
        let asst_msg = Message { role: MessageRole::Assistant, content: response.content.clone() };
        self.sessions.append_message(user_id, session_id, asst_msg).await?;

        Ok(response.content)
    }
}
