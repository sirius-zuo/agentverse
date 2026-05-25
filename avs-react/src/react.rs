//! ReAct strategy implementation.
//!
//! Implements the ReAct pattern: Think → Act → Observe → Think...
//! Uses the CycleSkeleton for utility methods and the shared cycle loop.
//!
//! NOTE: This file is a stub pending Task 4 rewrite. The full implementation
//! will use CycleSkeleton without M generic and the new RunStrategy::run API.

use super::cycle::CycleSkeleton;
use agentverse::{AgentError, LlmRunner, Message, PromptRegistry};
use agentverse_tools::ToolRegistry;
use std::sync::Arc;

/// The high-level ReAct strategy interface.
///
/// Users interact with this, not CycleSkeleton directly.
pub struct ReActStrategy {
    skeleton: CycleSkeleton,
}

impl ReActStrategy {
    /// Create a new ReAct strategy.
    pub fn new(
        runner: Arc<LlmRunner>,
        prompts: Arc<PromptRegistry>,
        tools: Arc<ToolRegistry>,
        max_iterations: usize,
    ) -> Self {
        Self {
            skeleton: CycleSkeleton::new(runner, prompts, tools, max_iterations),
        }
    }

    /// Return a reference to the underlying cycle skeleton.
    pub fn skeleton(&self) -> &CycleSkeleton {
        &self.skeleton
    }
}

#[async_trait::async_trait]
impl agentverse::RunStrategy for ReActStrategy {
    async fn run(&self, _messages: Vec<Message>) -> Result<String, AgentError> {
        todo!("Task 4: implement full ReAct loop using CycleSkeleton")
    }
}
