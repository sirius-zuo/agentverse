use super::{Agent, HitlConfig};
use agentverse::memory::LongtermMemory;
use agentverse::{LlmRunner, PromptRegistry, RunStrategy};
use agentverse_session::{SessionManager, SessionMemory};
use agentverse_skill::SkillConfig;
use agentverse_tools::ToolRegistry;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Builds an `Agent`. Required arguments are constructor parameters;
/// everything optional is a chainable `.with_*()` method.
///
/// ```ignore
/// Agent::builder(runner, tools, prompts, session_memory, strategy)
///     .with_http_server()
///     .with_skills(skills)
///     .build();
/// ```
pub struct AgentBuilder {
    runner: Arc<LlmRunner>,
    tools: Arc<ToolRegistry>,
    prompts: Arc<PromptRegistry>,
    session_memory: Arc<dyn SessionMemory>,
    strategy: Arc<dyn RunStrategy>,
    enable_http_server: bool,
    longterm_memory: Option<Arc<dyn LongtermMemory>>,
    skills: Option<SkillConfig>,
    hitl: Option<HitlConfig>,
    cleanup_config: Option<crate::workers::CleanupConfig>,
}

impl AgentBuilder {
    pub(super) fn new(
        runner: Arc<LlmRunner>,
        tools: Arc<ToolRegistry>,
        prompts: Arc<PromptRegistry>,
        session_memory: Arc<dyn SessionMemory>,
        strategy: Arc<dyn RunStrategy>,
    ) -> Self {
        Self {
            runner,
            tools,
            prompts,
            session_memory,
            strategy,
            enable_http_server: false,
            longterm_memory: None,
            skills: None,
            hitl: None,
            cleanup_config: None,
        }
    }

    pub fn with_http_server(mut self) -> Self {
        self.enable_http_server = true;
        self
    }

    pub fn with_longterm_memory(mut self, longterm_memory: Arc<dyn LongtermMemory>) -> Self {
        self.longterm_memory = Some(longterm_memory);
        self
    }

    pub fn with_skills(mut self, skills: SkillConfig) -> Self {
        self.skills = Some(skills);
        self
    }

    pub fn with_hitl(mut self, hitl: HitlConfig) -> Self {
        self.hitl = Some(hitl);
        self
    }

    pub fn with_cleanup_config(mut self, config: crate::workers::CleanupConfig) -> Self {
        self.cleanup_config = Some(config);
        self
    }

    pub fn build(self) -> Arc<Agent> {
        let session_memory_for_workers = Arc::clone(&self.session_memory);
        let agent = Arc::new(Agent {
            runner: self.runner,
            tools: self.tools,
            prompts: self.prompts,
            sessions: Arc::new(SessionManager::new(self.session_memory)),
            strategy: self.strategy,
            cache_memory: Mutex::new(HashMap::new()),
            buffer_ttl: Duration::from_secs(300),
            longterm_memory: self.longterm_memory,
            skills: self.skills,
            hitl: self.hitl,
            cleanup_config: self.cleanup_config.unwrap_or_default(),
        });

        agent.spawn_background_workers(session_memory_for_workers);

        #[cfg(feature = "http")]
        if self.enable_http_server {
            crate::http::spawn_server(Arc::clone(&agent));
        }
        #[cfg(not(feature = "http"))]
        let _ = self.enable_http_server;

        agent
    }
}

impl Agent {
    #[allow(clippy::too_many_arguments)]
    pub fn builder(
        runner: Arc<LlmRunner>,
        tools: Arc<ToolRegistry>,
        prompts: Arc<PromptRegistry>,
        session_memory: Arc<dyn SessionMemory>,
        strategy: Arc<dyn RunStrategy>,
    ) -> AgentBuilder {
        AgentBuilder::new(runner, tools, prompts, session_memory, strategy)
    }
}
