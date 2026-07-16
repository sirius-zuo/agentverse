use super::{Agent, HitlConfig};
use agentverse::{LlmRunner, PromptRegistry, RunStrategy};
use agentverse_hitl::{HitlPolicy, InMemoryQueue};
use agentverse_memory::{LongtermMemory, WorkingMemory};
use agentverse_router::StrategyRouter;
use agentverse_session::{SessionManager, SessionMemory};
use agentverse_skill::SkillConfig;
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use std::time::Duration;

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
    strategy_router: Option<StrategyRouter>,
    enable_http_server: bool,
    longterm_memory: Option<Arc<dyn LongtermMemory>>,
    working_memory: Option<Arc<dyn WorkingMemory>>,
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
            strategy_router: None,
            enable_http_server: false,
            longterm_memory: None,
            working_memory: None,
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

    pub fn with_working_memory(mut self, wm: Arc<dyn WorkingMemory>) -> Self {
        self.working_memory = Some(wm);
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

    /// Route each session-aware invocation to a freshly constructed strategy.
    /// Without this option, the supplied fixed strategy remains in use.
    pub fn with_strategy_router(mut self, router: StrategyRouter) -> Self {
        self.strategy_router = Some(router);
        self
    }

    pub fn with_cleanup_config(mut self, config: crate::workers::CleanupConfig) -> Self {
        self.cleanup_config = Some(config);
        self
    }

    pub fn build(self) -> Arc<Agent> {
        let hitl = match self.hitl {
            Some(hitl) => Some(hitl),
            None => self.skills.as_ref().and_then(|skills| {
                let trusted_system_skills = skills.trusted_system_skills();
                let has_hitl_declarations = trusted_system_skills.iter().any(|skill| {
                    !skill.hitl_tools.is_empty()
                        || skill.phase_gate
                        || !skill.checkpoints.is_empty()
                });
                has_hitl_declarations.then(|| HitlConfig {
                    policy: HitlPolicy::from_system_skills(trusted_system_skills),
                    queue: Arc::new(InMemoryQueue::new()),
                })
            }),
        };
        let session_memory_for_workers = Arc::clone(&self.session_memory);
        let agent = Arc::new(Agent {
            runner: self.runner,
            tools: self.tools,
            prompts: self.prompts,
            sessions: Arc::new(SessionManager::new(self.session_memory)),
            strategy: self.strategy,
            strategy_router: self.strategy_router,
            working_memory: self.working_memory.unwrap_or_else(|| {
                Arc::new(agentverse_memory::CacheMemory::new(Duration::from_secs(
                    300,
                )))
            }),
            longterm_memory: self.longterm_memory,
            skills: self.skills,
            hitl,
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
