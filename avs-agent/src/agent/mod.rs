use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentverse::memory::Message;
use agentverse::{LlmRunner, PromptRegistry, RunStrategy};
use agentverse_hitl::InterruptKind;
use agentverse_memory::LongtermMemory;
use agentverse_session::{SessionId, SessionManager, SessionMemoryError};
use agentverse_skill::{SkillConfig, SkillError};
use agentverse_tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use serde_json;
use tokio::sync::Mutex;
use uuid::Uuid;

mod builder;
mod invoke;
mod resume;
mod routing;
mod sessions;
mod workers;

pub use builder::AgentBuilder;
pub use routing::parse_phase_transition;

struct CacheMemory {
    messages: Vec<Message>,
    last_used: Instant,
}

/// Result of a skill phase transition parsed from an agent's output.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseTransition {
    pub next_skill: String,
    pub summary: String,
    pub deliverable: String,
}

/// Output of a single agent invocation.
/// The output of an agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentOutput {
    /// The agent completed successfully with this string response.
    Done(String),
    /// The agent was interrupted by HITL.
    /// The `kind` field matches what was submitted to the ApprovalQueue, so callers
    /// can display which interrupt type fired without re-querying the queue.
    Interrupted {
        approval_id: Uuid,
        kind: InterruptKind,
    },
}

impl std::fmt::Display for AgentOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentOutput::Done(text) => write!(f, "{text}"),
            AgentOutput::Interrupted { approval_id, kind } => {
                write!(
                    f,
                    "HITL interruption: approval_id={approval_id}, kind={kind:?}"
                )
            }
        }
    }
}

/// HITL configuration attached to an agent at construction time.
pub struct HitlConfig {
    pub policy: agentverse_hitl::HitlPolicy,
    pub queue: std::sync::Arc<dyn agentverse_hitl::ApprovalQueue>,
}

/// Result of advance_phase when HITL is configured.
pub enum PhaseAdvanceResult {
    /// Transition was applied immediately (no phase gate for this skill).
    Advanced(PhaseTransition),
    /// Transition is pending human approval; approval_id matches the queue entry.
    Pending { approval_id: Uuid },
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
    skills: Option<SkillConfig>,
    hitl: Option<HitlConfig>,
    cleanup_config: crate::workers::CleanupConfig,
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::Config;
    use agentverse_session::SqliteSessionMemory;
    use agentverse_strategy::{build, StrategyKind};

    #[tokio::test]
    async fn agent_output_done_variant_carries_string() {
        let out = AgentOutput::Done("hello".to_string());
        assert!(matches!(out, AgentOutput::Done(ref s) if s == "hello"));
    }

    #[test]
    fn hitl_config_can_be_constructed() {
        use agentverse_hitl::{HitlPolicy, InMemoryQueue};
        let _cfg = HitlConfig {
            policy: HitlPolicy::new(),
            queue: Arc::new(InMemoryQueue::new()) as Arc<dyn agentverse_hitl::ApprovalQueue>,
        };
    }

    #[test]
    fn phase_advance_result_pending_carries_approval_id() {
        let id = uuid::Uuid::new_v4();
        let r = PhaseAdvanceResult::Pending { approval_id: id };
        assert!(matches!(r, PhaseAdvanceResult::Pending { approval_id } if approval_id == id));
    }

    #[tokio::test]
    async fn agent_builder_accepts_custom_cleanup_config() {
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
        let config = crate::workers::CleanupConfig {
            message_retention: std::time::Duration::from_secs(3600),
            session_retention: std::time::Duration::from_secs(86400),
            poll_interval: std::time::Duration::from_secs(60),
        };
        // Proves with_cleanup_config exists, is chainable with the other
        // with_* methods, and that build() succeeds when it's used — the
        // real end-to-end proof that the worker actually HONORS this
        // config lives in avs-agent/src/workers.rs's
        // cleanup_worker_deletes_ended_sessions_past_retention.
        let _agent = Agent::builder(runner, tools, prompts, session_memory, strategy)
            .with_cleanup_config(config)
            .build();
    }
}
