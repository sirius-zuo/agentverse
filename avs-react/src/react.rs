//! ReAct strategy implementation.
//!
//! Implements the ReAct pattern: Think → Act → Observe → Think...
//! Uses the CycleSkeleton for utility methods and the shared cycle loop.

use super::cycle::{CycleAction, CycleSkeleton};
use super::parse::parse_response;
use agentverse::{AgentError, LlmRunner, Memory, Message, ModelError, PromptRegistry};
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

/// The high-level ReAct strategy interface.
///
/// Users interact with this, not CycleSkeleton directly.
pub struct ReActStrategy {
    skeleton: CycleSkeleton,
    #[allow(dead_code)]
    memory: Arc<Mutex<dyn Memory>>,
}

impl ReActStrategy {
    /// Create a new ReAct strategy.
    pub fn new(
        runner: Arc<LlmRunner>,
        prompts: Arc<PromptRegistry>,
        tools: Arc<ToolRegistry>,
        memory: Arc<Mutex<dyn Memory>>,
        max_iterations: usize,
    ) -> Self {
        Self {
            skeleton: CycleSkeleton::new(runner, prompts, tools, max_iterations),
            memory,
        }
    }
}

#[async_trait::async_trait]
impl agentverse::RunStrategy for ReActStrategy {
    async fn run(&self, messages: Vec<Message>) -> Result<String, AgentError> {
        let mut buf = self.skeleton.prepare_buffer(messages);
        let mut iteration = 0usize;
        let mut pending_answer: Option<String> = None;

        loop {
            if iteration >= self.skeleton.max_iterations() {
                return Err(AgentError::Model(ModelError::Timeout(format!(
                    "Max iterations ({}) reached",
                    self.skeleton.max_iterations()
                ))));
            }
            iteration += 1;

            let response = self.skeleton.runner.invoke(buf.clone()).await?;
            self.skeleton.check_output_guardrail(&response.content)?;

            let action = parse_response(&response.content);

            match action {
                CycleAction::Continue { thought } => {
                    if let Some(saved) = pending_answer.take() {
                        info!(iteration, "Strategy completed (nudge fallback)");
                        return Ok(saved);
                    }
                    info!(iteration, action = "continue", "Thought only, continuing");
                    pending_answer = Some(thought.clone());
                    buf.push(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                    buf.push(Message {
                        role: agentverse::MessageRole::User,
                        content: "Either call a tool (Action: / Action Input:) or give your final answer (Answer: ...).".to_string(),
                    });
                }
                CycleAction::ToolCall { tool_name, args } => {
                    pending_answer = None;
                    buf.push(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: response.content.clone(),
                    });
                    let result = self.skeleton.execute_tool(&tool_name, args).await?;
                    info!(iteration, action = "tool_call", tool = %tool_name, "Tool executed");
                    buf.push(Message {
                        role: agentverse::MessageRole::User,
                        content: format!("Tool: {}\nResult: {}", tool_name, result),
                    });
                }
                CycleAction::Done { answer } => {
                    info!(iteration, "Strategy completed");
                    return Ok(answer);
                }
                CycleAction::Error { message } => {
                    tracing::error!(iteration, error = %message, "Strategy error");
                    return Err(AgentError::Model(ModelError::InvalidResponse(message)));
                }
            }
        }
    }
}
