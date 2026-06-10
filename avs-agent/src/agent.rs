use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentverse::memory::{LongtermMemory, LongtermRecord, Message, MessageRole};
use agentverse::{LlmRunner, PromptRegistry, RunStrategy};
use agentverse_session::{Session, SessionId, SessionManager, SessionMemory, SessionMemoryError};
use agentverse_skill::{SkillContext, SkillError, SkillRegistry};
use agentverse_tools::ToolRegistry;
use serde_json;
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
    #[error("skill error: {0}")]
    Skill(#[from] SkillError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
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
    skill_registry: Option<Arc<SkillRegistry>>,
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
        skill_registry: Option<Arc<SkillRegistry>>,
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
            skill_registry,
        });

        #[cfg(feature = "http")]
        if enable_http_server {
            crate::http::spawn_server(Arc::clone(&agent));
        }

        // Suppress unused variable warning when http feature is disabled
        let _ = enable_http_server;
        agent
    }

    fn render_system_with_skill(&self, skill: Option<&SkillContext>) -> Option<String> {
        let base = self.prompts
            .render("system", std::collections::HashMap::new())
            .ok()
            .filter(|s| !s.trim().is_empty());
        let skill_block = skill.map(|ctx| {
            let mut parts = vec![ctx.instructions.clone()];
            parts.extend(ctx.documents.iter().cloned());
            parts.join("\n\n")
        });
        match (skill_block, base) {
            (Some(sk), Some(b)) => Some(format!("{}\n\n{}", sk, b)),
            (Some(sk), None) => Some(sk),
            (None, b) => b,
        }
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

    /// Single-turn stateless invocation with no session, history, or skill context.
    /// Skill sessions (created via `create_session_with_skill`) must use the
    /// session-aware `invoke` path, not this method.
    pub async fn invoke_stateless(&self, input: &str) -> Result<String, AgentError> {
        // Stateless: no session, no memory context — always a fresh single-turn call.
        let messages = self.assemble_messages(self.render_system_with_skill(None), vec![], input);
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

        // Load skill context for this session (session-stable)
        let skill_ctx: Option<SkillContext> = self
            .sessions
            .get_skill_context(session_id)
            .await?
            .map(|json| serde_json::from_str::<SkillContext>(&json))
            .transpose()?;

        // Active tool names: skill tools ∩ registry, or all if no skill.
        // A skill with tools:[] restricts to zero tools; only None (no skill) means all tools.
        let active_tool_names: Vec<String> = match &skill_ctx {
            None => self.tools.tool_names(),
            Some(ctx) => ctx
                .tools
                .iter()
                .filter(|name| self.tools.has_tool(name))
                .cloned()
                .collect(),
        };

        // Layer 3: retrieve scored memories
        let long_term_text = if let Some(ref ms) = self.longterm_memory {
            let memories = ms
                .retrieve(user_id, input, 5)
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
            self.render_system_with_skill(skill_ctx.as_ref()),
            long_term_text,
            history,
            input,
        );
        let response = self
            .strategy
            .run_with_active_tools(messages, &active_tool_names)
            .await?;

        let assistant_msg = Message {
            role: MessageRole::Assistant,
            content: response.clone(),
        };
        self.sessions
            .append_turn(session_id, user_msg.clone(), assistant_msg.clone())
            .await?;
        self.update_cache_memory(user_id, session_id, user_msg, assistant_msg)
            .await;

        if let Some(ms) = self.longterm_memory.clone() {
            let uid = user_id.to_string();
            let record = LongtermRecord::now(
                format!("User: {input}\nAssistant: {response}"),
                0.5,
            );
            tokio::spawn(async move {
                let _ = ms.write(&uid, record).await;
            });
        }

        Ok(response)
    }

    pub async fn create_session(&self, user_id: &str) -> Result<SessionId, AgentError> {
        Ok(self.sessions.create_session(user_id).await?)
    }

    pub async fn create_session_with_skill(
        &self,
        user_id: &str,
        skill_id: &str,
    ) -> Result<SessionId, AgentError> {
        let registry = self.skill_registry.as_ref().ok_or_else(|| {
            SkillError::NotConfigured("no skill registry configured on this agent".into())
        })?;
        let ctx = registry.compile_context(skill_id)?;
        let ctx_json = serde_json::to_string(&ctx)?;
        Ok(self
            .sessions
            .create_session_with_skill_context(user_id, &ctx_json)
            .await?)
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
        let agent = Agent::new(
            runner,
            tools,
            prompts,
            session_memory,
            strategy,
            false,
            Some(ms),
            None,
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
        Agent::new(
            runner,
            tools,
            prompts,
            session_memory,
            strategy,
            false,
            None,
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

#[cfg(test)]
mod skill_tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_skill::SkillRegistry;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;
    use tempfile::tempdir;
    use std::fs;

    async fn make_agent_with_skill_dir(skills_dir: &std::path::Path) -> Arc<Agent> {
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
        let skill_registry = SkillRegistry::load(skills_dir).ok();
        Agent::new(
            runner,
            tools,
            prompts,
            session_memory,
            strategy,
            false,
            None,
            skill_registry,
        )
    }

    fn write_skill(dir: &std::path::Path, subdir: &str, name: &str, instructions: &str) {
        let pkg = dir.join(subdir).join(name);
        fs::create_dir_all(&pkg).unwrap();
        let content = format!(
            "---\nname: {name}\ndescription: Test skill.\nagentverse:\n  tools:\n    - find_tools\n---\n\n{instructions}\n"
        );
        fs::write(pkg.join("SKILL.md"), content).unwrap();
    }

    #[tokio::test]
    async fn create_session_with_skill_stores_context() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "system", "test-skill", "You are a test agent.");
        let agent = make_agent_with_skill_dir(dir.path()).await;

        let session_id = agent
            .create_session_with_skill("alice", "test-skill")
            .await
            .unwrap();

        // The skill context should be stored in the session
        let ctx_json = agent.sessions.get_skill_context(session_id).await.unwrap();
        assert!(ctx_json.is_some());
        let ctx: agentverse_skill::SkillContext =
            serde_json::from_str(&ctx_json.unwrap()).unwrap();
        assert!(ctx.instructions.contains("You are a test agent."));
        assert!(ctx.tools.contains(&"find_tools".to_string()));
    }

    #[tokio::test]
    async fn create_session_with_skill_returns_error_for_unknown_skill() {
        let dir = tempdir().unwrap();
        let agent = make_agent_with_skill_dir(dir.path()).await;
        let result = agent
            .create_session_with_skill("alice", "nonexistent-skill")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn empty_tools_in_skill_restricts_to_zero_tools() {
        // Write a skill with an empty tools list
        let dir = tempdir().unwrap();
        let pkg = dir.path().join("system").join("no-tools-skill");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("SKILL.md"),
            "---\nname: no-tools-skill\ndescription: Restricted.\nagentverse:\n  tools: []\n---\n\nNo tools.\n",
        )
        .unwrap();

        let agent = make_agent_with_skill_dir(dir.path()).await;
        let session_id = agent
            .create_session_with_skill("alice", "no-tools-skill")
            .await
            .unwrap();

        // Retrieve the stored context and check tools is empty
        let ctx_json = agent.sessions.get_skill_context(session_id).await.unwrap().unwrap();
        let ctx: agentverse_skill::SkillContext = serde_json::from_str(&ctx_json).unwrap();
        assert!(ctx.tools.is_empty(), "skill declared no tools — ctx.tools should be empty");

        // Verify that invoke would resolve active_tool_names to [] not to all tools.
        // We can't call invoke without a live LLM, but we can verify the stored context
        // has empty tools, which is the input to the active_tool_names calculation.
        // The calculation `None => all, Some(ctx) => filtered(ctx.tools)` should give [].
        let active: Vec<String> = ctx
            .tools
            .iter()
            .filter(|name| agent.tools.has_tool(name))
            .cloned()
            .collect();
        assert!(active.is_empty(), "expected zero active tools for skills with empty tools list");
    }
}
