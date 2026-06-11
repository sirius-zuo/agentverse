use agentverse::{AgentError, ToolError, UsageStats};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub answer: String,
    pub usage: UsageStats,
    pub steps: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SubAgentError {
    #[error("depth limit exceeded: subagents cannot spawn subagents")]
    DepthExceeded,

    #[error("step budget exhausted after {steps} steps")]
    StepBudgetExceeded { steps: usize },

    #[error("token budget exceeded: used {used}, limit {limit}")]
    TokenBudgetExceeded { used: u32, limit: u32 },

    #[error("timeout after {elapsed:?}")]
    Timeout { elapsed: Duration },

    #[error("llm error: {0}")]
    Llm(#[from] AgentError),

    #[error("tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("subagent panicked: {0}")]
    Panic(String),
}
