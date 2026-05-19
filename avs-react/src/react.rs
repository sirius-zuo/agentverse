//! ReAct strategy implementation.
//!
//! Implements the ReAct pattern: Think → Act → Observe → Think...
//! Uses the CycleSkeleton for utility methods and the shared cycle loop.

use super::cycle::{CycleAction, CycleSkeleton};
use super::parse::parse_response;
use agentverse::{AgentError, ModelProvider, PromptRegistry};
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::Mutex;

/// The high-level ReAct strategy interface.
///
/// Users interact with this, not CycleSkeleton directly.
pub struct ReActStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    skeleton: CycleSkeleton<P, M>,
}

impl<P, M> ReActStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    /// Create a new ReAct strategy.
    pub fn new(
        prompt_registry: Arc<PromptRegistry>,
        model: Arc<P>,
        tools: ToolRegistry,
        memory: Arc<Mutex<M>>,
        max_iterations: usize,
    ) -> Self {
        Self {
            skeleton: CycleSkeleton::new(prompt_registry, model, tools, memory, max_iterations),
        }
    }

    /// Execute the ReAct loop.
    ///
    /// The loop runs until the model returns an answer, an error occurs,
    /// or max iterations is reached.
    pub async fn run(&mut self, input: String) -> Result<agentverse::CycleResult, AgentError> {
        // Insert the react preamble as the first message when a react.j2 file
        // Reset per-run so the iteration limit applies to this question, not the session total.
        self.skeleton.reset_iteration();

        // was loaded.  Idempotent — does nothing on subsequent calls.
        self.skeleton.prime_react_preamble().await;

        self.skeleton
            .memory()
            .lock()
            .await
            .append(agentverse::Message {
                role: agentverse::memory::MessageRole::User,
                content: input,
            });

        loop {
            if self.skeleton.current_iteration() >= self.skeleton.max_iterations() {
                return Err(AgentError::Model(agentverse::ModelError::Timeout(format!(
                    "Max iterations ({}) reached",
                    self.skeleton.max_iterations()
                ))));
            }

            let _iter = self.skeleton.next_iteration();

            let request = self.skeleton.build_request_with_guardrails().await?;

            let response = self.skeleton.model().generate(request).await?;

            self.skeleton.accumulate_usage(response.usage);
            self.skeleton.check_output_guardrail(&response.content)?;

            let action = parse_response(&response.content);

            match action {
                CycleAction::Continue { thought } => {
                    self.skeleton
                        .memory()
                        .lock()
                        .await
                        .append(agentverse::Message {
                            role: agentverse::memory::MessageRole::Assistant,
                            content: format!("Thought: {}", thought),
                        });
                }
                CycleAction::ToolCall { tool_name, args } => {
                    // Save the model's own reasoning so the next iteration has full context.
                    self.skeleton
                        .memory()
                        .lock()
                        .await
                        .append(agentverse::Message {
                            role: agentverse::memory::MessageRole::Assistant,
                            content: response.content.clone(),
                        });
                    let result = self.skeleton.execute_tool(&tool_name, args).await?;
                    // Use User role for tool observations — text-based ReAct does not use
                    // native tool calling, so "tool" role (which requires tool_call_id) breaks
                    // chat templates on local models.
                    self.skeleton
                        .memory()
                        .lock()
                        .await
                        .append(agentverse::Message {
                            role: agentverse::memory::MessageRole::User,
                            content: format!("Tool: {}\nResult: {}", tool_name, result),
                        });
                }
                CycleAction::Done { answer } => {
                    self.skeleton
                        .memory()
                        .lock()
                        .await
                        .append(agentverse::Message {
                            role: agentverse::memory::MessageRole::Assistant,
                            content: answer.clone(),
                        });
                    return Ok(agentverse::CycleResult {
                        answer,
                        total_usage: self.skeleton.total_usage(),
                    });
                }
                CycleAction::Error { message } => {
                    return Err(AgentError::Model(agentverse::ModelError::InvalidResponse(
                        message,
                    )));
                }
            }
        }
    }

    /// Return a reference to the underlying cycle skeleton.
    pub fn skeleton(&self) -> &CycleSkeleton<P, M> {
        &self.skeleton
    }
}

#[async_trait::async_trait]
impl<P, M> agentverse::RunStrategy for ReActStrategy<P, M>
where
    P: agentverse::ModelProvider + Send + Sync + 'static,
    M: agentverse::Memory + Send + 'static,
{
    async fn process(&mut self, input: String) -> Result<String, agentverse::AgentError> {
        self.run(input).await.map(|r| r.answer)
    }
}
