use super::{Agent, AgentError};
use agentverse::memory::Message;
use agentverse_session::{Session, SessionId};

impl Agent {
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
        self.sessions.end_session(session_id).await?;
        // Layer-1 cascade: evict working buffer immediately on session delete
        let key = (user_id.to_string(), session_id);
        self.cache_memory.lock().await.remove(&key);
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use agentverse::memory::{LongtermMemory, LongtermRecord, MemoryError, ScoredMemory};
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

    struct NoopMemoryStore;
    #[async_trait::async_trait]
    impl LongtermMemory for NoopMemoryStore {
        async fn write(&self, _: &str, _: LongtermRecord) -> Result<(), MemoryError> {
            Ok(())
        }
        async fn retrieve(
            &self,
            _: &str,
            _: &str,
            _: usize,
        ) -> Result<Vec<ScoredMemory>, MemoryError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn agent_with_memory_store_creates_session_normally() {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::OpenAI {
                    model_name: "test".to_string(),
                    api_key: "sk-test".to_string(),
                    base_url: Some("http://127.0.0.1:1/v1".to_string()),
                },
                max_messages: 10,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap(),
        );
        let tools = ToolRegistry::new();
        let prompts = Arc::new(PromptRegistry::new());
        let strategy = build(
            StrategyKind::React,
            Arc::clone(&runner),
            Arc::clone(&prompts),
            Arc::clone(&tools),
            3,
        );
        let session_memory = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
        let ms: Arc<dyn LongtermMemory> = Arc::new(NoopMemoryStore);
        let agent = super::Agent::builder(runner, tools, prompts, session_memory, strategy)
            .with_longterm_memory(ms)
            .build();
        let sid = agent.create_session("alice").await.unwrap();
        assert!(agent.get_session("alice", sid).await.unwrap().is_some());
    }
}
