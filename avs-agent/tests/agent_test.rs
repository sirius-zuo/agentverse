use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use agentverse::memory::{Message, MessageRole};
use agentverse_session::{Session, SessionId, SessionMemory, SessionMemoryError, SessionStatus};
use async_trait::async_trait;

#[derive(Default)]
struct FakeStore {
    sessions: Mutex<HashMap<SessionId, Session>>,
    messages: Mutex<HashMap<SessionId, Vec<Message>>>,
    watermarks: Mutex<HashMap<SessionId, i64>>,
}

#[async_trait]
impl SessionMemory for FakeStore {
    async fn create(&self, user_id: &str) -> Result<Session, SessionMemoryError> {
        let session = Session::new(user_id);
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id, session.clone());
        Ok(session)
    }

    async fn get(&self, session_id: SessionId) -> Result<Option<Session>, SessionMemoryError> {
        Ok(self.sessions.lock().unwrap().get(&session_id).cloned())
    }

    async fn update_status(
        &self,
        session_id: SessionId,
        status: SessionStatus,
    ) -> Result<(), SessionMemoryError> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(&session_id)
            .ok_or(SessionMemoryError::NotFound(session_id))?;
        session.status = status;
        Ok(())
    }

    async fn list_by_user(&self, user_id: &str) -> Result<Vec<Session>, SessionMemoryError> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|session| session.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn append_message(
        &self,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionMemoryError> {
        if !self.sessions.lock().unwrap().contains_key(&session_id) {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        self.messages
            .lock()
            .unwrap()
            .entry(session_id)
            .or_default()
            .push(message);
        Ok(())
    }

    async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<(), SessionMemoryError> {
        self.append_message(session_id, user_message).await?;
        self.append_message(session_id, assistant_message).await
    }

    async fn load_messages(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionMemoryError> {
        if !self.sessions.lock().unwrap().contains_key(&session_id) {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        Ok(self
            .messages
            .lock()
            .unwrap()
            .get(&session_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionMemoryError> {
        Ok(*self
            .watermarks
            .lock()
            .unwrap()
            .get(&session_id)
            .unwrap_or(&0))
    }

    async fn advance_watermark(
        &self,
        session_id: SessionId,
        new_watermark: i64,
    ) -> Result<(), SessionMemoryError> {
        let mut wm = self.watermarks.lock().unwrap();
        let entry = wm.entry(session_id).or_insert(0);
        if new_watermark > *entry {
            *entry = new_watermark;
        }
        Ok(())
    }

    async fn load_messages_above_watermark(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<(i64, agentverse::memory::Message)>, SessionMemoryError> {
        let wm = self.get_watermark(session_id).await?;
        let msgs = self.load_messages(session_id).await?;
        Ok(msgs
            .into_iter()
            .enumerate()
            .map(|(i, m)| (i as i64 + 1, m))
            .filter(|(seq, _)| *seq > wm)
            .collect())
    }

    async fn cleanup_expired_messages(
        &self,
        _session_id: SessionId,
        _cutoff_ts: i64,
        _watermark: i64,
    ) -> Result<u64, SessionMemoryError> {
        Ok(0)
    }

    async fn list_sessions_needing_maintenance(&self) -> Result<Vec<Session>, SessionMemoryError> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn session_manager_rejects_wrong_user_before_llm_call() {
    let session_memory = Arc::new(FakeStore::default());
    let session = session_memory.create("alice").await.unwrap();
    let manager = agentverse_session::SessionManager::new(session_memory);

    let err = manager.assert_owner("bob", session.id).await.unwrap_err();
    assert!(matches!(err, SessionMemoryError::NotFound(id) if id == session.id));
}

#[tokio::test]
async fn append_turn_contract_preserves_user_then_assistant_order() {
    let session_memory = Arc::new(FakeStore::default());
    let session = session_memory.create("alice").await.unwrap();

    session_memory
        .append_turn(
            session.id,
            Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            },
            Message {
                role: MessageRole::Assistant,
                content: "hi".to_string(),
            },
        )
        .await
        .unwrap();

    let messages = session_memory.load_messages(session.id).await.unwrap();
    assert_eq!(messages[0].content, "hello");
    assert_eq!(messages[1].content, "hi");
}

#[test]
fn agent_with_subagent_executor_registers_spawn_subagent_tool() {
    use agentverse::ConnectionManager;
    use agentverse::PromptRegistry;
    use agentverse_subagent::SubAgentExecutor;
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

    let tools = ToolRegistry::new();
    assert!(!tools.has_tool("spawn_subagent"));

    let cm = Arc::new(ConnectionManager::anthropic(
        "http://127.0.0.1:1",
        "claude-sonnet-4-6",
        "test-key",
    ));
    let executor = Arc::new(SubAgentExecutor::new(
        cm,
        Arc::clone(&tools),
        Arc::new(PromptRegistry::new()),
    ));

    SubAgentExecutor::register_tool(&executor, &tools);
    assert!(tools.has_tool("spawn_subagent"));
}
