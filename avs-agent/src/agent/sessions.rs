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

    /// Deletes every session this user has in L2 (`SessionMemory`) and evicts
    /// all of their entries from the L1 working-buffer cache. Does NOT touch
    /// L3 (`LongtermMemory`) — deletion there is explicitly outside
    /// agentverse's responsibility (L3 data may serve purposes beyond this
    /// agent's own runtime, e.g. training corpora).
    ///
    /// No `assert_owner` check here, unlike `end_session`/`get_session`: this
    /// method never takes a caller-supplied `session_id` to verify against
    /// `user_id` — every session it touches comes from `list_sessions(user_id)`,
    /// a query already scoped by the trusted `user_id` parameter itself. Same
    /// trust model as `create_session`/`list_sessions`, not `end_session`'s
    /// (which must check ownership because it receives an untrusted `session_id`
    /// that could belong to any user).
    pub async fn delete_all_user_data(&self, user_id: &str) -> Result<(), AgentError> {
        let sessions = self.sessions.list_sessions(user_id).await?;
        for session in &sessions {
            self.sessions.delete_session(session.id).await?;
        }
        let mut cache = self.cache_memory.lock().await;
        cache.retain(|(cached_user_id, _), _| cached_user_id != user_id);
        Ok(())
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
    use super::super::{Agent, CacheMemory};
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
                provider: agentverse::ProviderConfig::openai(
                    "test".to_string(),
                    "sk-test".to_string(),
                    Some("http://127.0.0.1:1/v1".to_string()),
                ),
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

    #[tokio::test]
    async fn delete_all_user_data_removes_l2_sessions_and_l1_cache() {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::openai(
                    "test".to_string(),
                    "sk-test".to_string(),
                    Some("http://127.0.0.1:1/v1".to_string()),
                ),
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
        let agent = Agent::builder(runner, tools, prompts, session_memory, strategy).build();

        let session_id = agent.create_session("alice").await.unwrap();

        // Populate L1 cache directly (no live LLM call needed) — CacheMemory
        // and Agent.cache_memory are private fields defined in
        // avs-agent/src/agent/mod.rs; this test module is a child of that
        // module (via `mod sessions;`) so it has the same private-field
        // access `end_session` already uses in this same file.
        agent.cache_memory.lock().await.insert(
            ("alice".to_string(), session_id),
            CacheMemory {
                messages: vec![agentverse::memory::Message {
                    role: agentverse::memory::MessageRole::User,
                    content: "cached turn".to_string(),
                }],
                last_used: std::time::Instant::now(),
            },
        );

        agent.delete_all_user_data("alice").await.unwrap();

        assert!(
            agent.get_session("alice", session_id).await.is_err(),
            "session must be gone from L2 after delete_all_user_data"
        );
        let cache = agent.cache_memory.lock().await;
        assert!(
            !cache.keys().any(|(uid, _)| uid == "alice"),
            "no L1 cache entry for this user should remain"
        );
    }
}
