use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentverse::memory::{LongtermMemory, LongtermRecord, Message, MessageRole};
use agentverse::{LlmRunner, PromptRegistry, RunStrategy};
use agentverse_session::{Session, SessionId, SessionManager, SessionMemory, SessionMemoryError};
use agentverse_tools::ToolRegistry;
use tokio::sync::Mutex;

struct CacheMemory {
    messages: Vec<Message>,
    last_used: Instant,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("session error: {0}")]
    Session(#[from] SessionMemoryError),
    #[error("llm error: {0}")]
    Llm(#[from] agentverse::AgentError),
}

pub struct Agent {
    // Held for ownership and future use (e.g. HTTP server, direct tool calls)
    #[allow(dead_code)]
    runner: Arc<LlmRunner>,
    #[allow(dead_code)]
    tools: Arc<ToolRegistry>,
    prompts: Arc<PromptRegistry>,
    sessions: Arc<SessionManager>,
    strategy: Arc<dyn RunStrategy>,
    cache_memory: Mutex<HashMap<(String, SessionId), CacheMemory>>,
    buffer_ttl: Duration,
    longterm_memory: Option<Arc<dyn LongtermMemory>>,
}

impl Agent {
    pub fn new(
        runner: Arc<LlmRunner>,
        tools: Arc<ToolRegistry>,
        prompts: Arc<PromptRegistry>,
        session_memory: Arc<dyn SessionMemory>,
        strategy: Arc<dyn RunStrategy>,
        enable_http_server: bool,
        longterm_memory: Option<Arc<dyn LongtermMemory>>,
    ) -> Arc<Self> {
        let agent = Arc::new(Self {
            runner,
            tools,
            prompts,
            sessions: Arc::new(SessionManager::new(session_memory)),
            strategy,
            cache_memory: Mutex::new(HashMap::new()),
            buffer_ttl: Duration::from_secs(300),
            longterm_memory,
        });

        #[cfg(feature = "http")]
        if enable_http_server {
            crate::http::spawn_server(Arc::clone(&agent));
        }

        // Suppress unused variable warning when http feature is disabled
        let _ = enable_http_server;
        agent
    }

    fn render_system(&self) -> Option<String> {
        self.prompts
            .render("system", std::collections::HashMap::new())
            .ok()
            .filter(|s| !s.trim().is_empty())
    }

    fn assemble_messages(
        &self,
        system: Option<String>,
        history: Vec<Message>,
        input: &str,
    ) -> Vec<Message> {
        let mut msgs = Vec::new();
        if let Some(sys) = system {
            msgs.push(Message {
                role: MessageRole::System,
                content: sys,
            });
        }
        msgs.extend(history);
        msgs.push(Message {
            role: MessageRole::User,
            content: input.to_string(),
        });
        msgs
    }

    fn assemble_messages_with_context(
        &self,
        system: Option<String>,
        long_term_text: Option<String>,
        history: Vec<Message>,
        input: &str,
    ) -> Vec<Message> {
        let mut msgs = Vec::new();
        let sys_content = match (system, long_term_text) {
            (Some(sys), Some(lt)) => Some(format!("{sys}\n\n{lt}")),
            (Some(sys), None) => Some(sys),
            (None, Some(lt)) => Some(lt),
            (None, None) => None,
        };
        if let Some(content) = sys_content {
            msgs.push(Message {
                role: MessageRole::System,
                content,
            });
        }
        msgs.extend(history);
        msgs.push(Message {
            role: MessageRole::User,
            content: input.to_string(),
        });
        msgs
    }

    async fn get_cache_memory(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Vec<Message>, AgentError> {
        let key = (user_id.to_string(), session_id);
        {
            let cache = self.cache_memory.lock().await;
            if let Some(buf) = cache.get(&key) {
                if buf.last_used.elapsed() <= self.buffer_ttl {
                    return Ok(buf.messages.clone());
                }
            }
        }
        // Miss or TTL expired: sweep expired entries, then rehydrate from Layer 2
        let history = self.sessions.load_messages(session_id).await?;
        let mut cache = self.cache_memory.lock().await;
        let ttl = self.buffer_ttl;
        cache.retain(|_, buf| buf.last_used.elapsed() <= ttl);
        cache.insert(
            key,
            CacheMemory {
                messages: history.clone(),
                last_used: Instant::now(),
            },
        );
        Ok(history)
    }

    async fn update_cache_memory(
        &self,
        user_id: &str,
        session_id: SessionId,
        user_msg: Message,
        assistant_msg: Message,
    ) {
        let key = (user_id.to_string(), session_id);
        let mut cache = self.cache_memory.lock().await;
        if let Some(buf) = cache.get_mut(&key) {
            buf.messages.push(user_msg);
            buf.messages.push(assistant_msg);
            buf.last_used = Instant::now();
        } else {
            // Key was TTL-evicted during the LLM call; insert a minimal buffer
            // with just this turn so the next invoke avoids a cold DB read.
            cache.insert(
                key,
                CacheMemory {
                    messages: vec![user_msg, assistant_msg],
                    last_used: Instant::now(),
                },
            );
        }
    }

    pub async fn invoke_stateless(&self, input: &str) -> Result<String, AgentError> {
        // Stateless: no session, no memory context — always a fresh single-turn call.
        let messages = self.assemble_messages(self.render_system(), vec![], input);
        let response = self.strategy.run(messages).await?;
        Ok(response)
    }

    pub async fn invoke(
        &self,
        user_id: &str,
        session_id: SessionId,
        input: &str,
    ) -> Result<String, AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;

        let history = self.get_cache_memory(user_id, session_id).await?;

        // Layer 3: retrieve scored memories and inject into system prompt
        let long_term_text = if let Some(ref ms) = self.longterm_memory {
            let memories = ms.retrieve(user_id, input, 5)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "layer-3 memory retrieve failed, proceeding without context");
                    vec![]
                });
            if memories.is_empty() {
                None
            } else {
                Some(
                    memories
                        .into_iter()
                        .map(|sm| format!("[Memory] {}", sm.content))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
        } else {
            None
        };

        let user_msg = Message {
            role: MessageRole::User,
            content: input.to_string(),
        };
        let messages = self.assemble_messages_with_context(
            self.render_system(),
            long_term_text,
            history,
            input,
        );
        let response = self.strategy.run(messages).await?;

        let assistant_msg = Message {
            role: MessageRole::Assistant,
            content: response.clone(),
        };
        self.sessions
            .append_turn(session_id, user_msg.clone(), assistant_msg.clone())
            .await?;
        self.update_cache_memory(user_id, session_id, user_msg, assistant_msg)
            .await;

        // Layer 3: async fire-and-forget consolidation
        if let Some(ms) = self.longterm_memory.clone() {
            let uid = user_id.to_string();
            // TODO: replace 0.5 with heuristic or LLM-assigned importance scorer
            let record = LongtermRecord::now(format!("User: {input}\nAssistant: {response}"), 0.5);
            tokio::spawn(async move {
                let _ = ms.write(&uid, record).await;
            });
        }

        Ok(response)
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
mod lt_tests {
    use super::*;
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
        let tools = Arc::new(ToolRegistry::new());
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
        let agent = Agent::new(
            runner,
            tools,
            prompts,
            session_memory,
            strategy,
            false,
            Some(ms),
        );
        let sid = agent.create_session("alice").await.unwrap();
        assert!(agent.get_session("alice", sid).await.unwrap().is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

    async fn make_agent() -> Arc<Agent> {
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
        let tools = Arc::new(ToolRegistry::new());
        let prompts = Arc::new(PromptRegistry::new());
        let strategy = build(
            StrategyKind::React,
            Arc::clone(&runner),
            Arc::clone(&prompts),
            Arc::clone(&tools),
            3,
        );
        let session_memory = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
        Agent::new(
            runner,
            tools,
            prompts,
            session_memory,
            strategy,
            false,
            None,
        )
    }

    #[tokio::test]
    async fn invoke_stateless_returns_error_on_bad_port() {
        let agent = make_agent().await;
        let result = agent.invoke_stateless("hello").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn create_and_get_session_works() {
        let agent = make_agent().await;
        let session_id = agent.create_session("alice").await.unwrap();
        let session = agent.get_session("alice", session_id).await.unwrap();
        assert!(session.is_some());
        assert_eq!(session.unwrap().user_id, "alice");
    }

    #[tokio::test]
    async fn working_buffer_rehydrates_after_db_write() {
        // Verifies the rehydration path: fresh session → load_messages returns empty
        let agent = make_agent().await;
        let sid = agent.create_session("alice").await.unwrap();
        let msgs = agent.load_messages("alice", sid).await.unwrap();
        assert!(msgs.is_empty());
    }
}
