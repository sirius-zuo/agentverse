use std::sync::Arc;

use agentverse::memory::{Message, MessageRole};
use agentverse::{LlmRunner, Memory, PromptRegistry, RunStrategy};
use agentverse_session::{Session, SessionId, SessionManager, SessionStore, SessionStoreError};
use agentverse_tools::ToolRegistry;
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("session error: {0}")]
    Session(#[from] SessionStoreError),
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
    #[allow(dead_code)]
    memory: Arc<Mutex<dyn Memory>>,
    sessions: Arc<SessionManager>,
    strategy: Arc<dyn RunStrategy>,
}

impl Agent {
    pub fn new(
        runner: Arc<LlmRunner>,
        tools: Arc<ToolRegistry>,
        prompts: Arc<PromptRegistry>,
        memory: Arc<Mutex<dyn Memory>>,
        store: Arc<dyn SessionStore>,
        strategy: Arc<dyn RunStrategy>,
        enable_http_server: bool,
    ) -> Arc<Self> {
        let agent = Arc::new(Self {
            runner,
            tools,
            prompts,
            memory,
            sessions: Arc::new(SessionManager::new(store)),
            strategy,
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

    pub async fn invoke_stateless(&self, input: &str) -> Result<String, AgentError> {
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

        let history = self.sessions.load_messages(session_id).await?;
        let user_msg = Message {
            role: MessageRole::User,
            content: input.to_string(),
        };

        let messages = self.assemble_messages(self.render_system(), history, input);
        let response = self.strategy.run(messages).await?;

        let assistant_msg = Message {
            role: MessageRole::Assistant,
            content: response.clone(),
        };
        self.sessions
            .append_turn(session_id, user_msg, assistant_msg)
            .await?;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_memory::SimpleMemory;
    use agentverse_session::SqliteSessionStore;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;
    use tokio::sync::Mutex;

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
        let memory: Arc<Mutex<dyn agentverse::Memory>> =
            Arc::new(Mutex::new(SimpleMemory::new(50)));
        let strategy = build(
            StrategyKind::React,
            Arc::clone(&runner),
            Arc::clone(&prompts),
            Arc::clone(&tools),
            Arc::clone(&memory),
            3,
        );
        let store = Arc::new(SqliteSessionStore::new("sqlite::memory:").await.unwrap());
        Agent::new(runner, tools, prompts, memory, store, strategy, false)
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
}
