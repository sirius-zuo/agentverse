use std::sync::Arc;

use agentverse::memory::{Message, MessageRole};
use agentverse::LlmRunner;
use agentverse_session::{Session, SessionId, SessionManager, SessionStore, SessionStoreError};

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("session error: {0}")]
    Session(#[from] SessionStoreError),
    #[error("llm error: {0}")]
    Llm(#[from] agentverse::AgentError),
}

pub struct Agent {
    runner: Arc<LlmRunner>,
    sessions: Arc<SessionManager>,
}

impl Agent {
    pub fn new(runner: Arc<LlmRunner>, store: Arc<dyn SessionStore>) -> Self {
        Self {
            runner,
            sessions: Arc::new(SessionManager::new(store)),
        }
    }

    pub fn from_parts(runner: Arc<LlmRunner>, sessions: Arc<SessionManager>) -> Self {
        Self { runner, sessions }
    }

    pub async fn create_session(&self, user_id: &str) -> Result<SessionId, AgentError> {
        Ok(self.sessions.create_session(user_id).await?)
    }

    pub async fn get_session(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Option<Session>, AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;
        Ok(self.sessions.get_session(session_id).await?)
    }

    pub async fn end_session(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<(), AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;
        Ok(self.sessions.end_session(session_id).await?)
    }

    pub async fn list_sessions(&self, user_id: &str) -> Result<Vec<Session>, AgentError> {
        Ok(self.sessions.list_sessions(user_id).await?)
    }

    pub async fn load_messages(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Vec<Message>, AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;
        Ok(self.sessions.load_messages(session_id).await?)
    }

    pub async fn invoke(
        &self,
        user_id: &str,
        session_id: SessionId,
        input: &str,
    ) -> Result<String, AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;

        let mut messages = self.sessions.load_messages(session_id).await?;
        let user_msg = Message {
            role: MessageRole::User,
            content: input.to_string(),
        };
        messages.push(user_msg.clone());

        let response = self.runner.invoke(messages).await?;
        let assistant_msg = Message {
            role: MessageRole::Assistant,
            content: response.content.clone(),
        };

        self.sessions
            .append_turn(session_id, user_msg, assistant_msg)
            .await?;

        Ok(response.content)
    }
}
