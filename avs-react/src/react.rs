//! ReAct strategy implementation.
//!
//! Implements the ReAct pattern: Think → Act → Observe → Think...
//! Uses the CycleSkeleton for utility methods and the shared cycle loop.

use super::cycle::{CycleAction, CycleSkeleton};
use super::parse::parse_response;
use agentverse::{AgentError, ConnectionManager, PromptRegistry};
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

/// The high-level ReAct strategy interface.
///
/// Users interact with this, not CycleSkeleton directly.
pub struct ReActStrategy<M>
where
    M: agentverse::Memory,
{
    skeleton: CycleSkeleton<M>,
    /// Thought saved when we issued the nudge. If the model responds with
    /// another Continue instead of Answer/Action, we return this thought
    /// directly — it was already a complete answer, just missing the label.
    pending_answer: Option<String>,
}

impl<M> ReActStrategy<M>
where
    M: agentverse::Memory,
{
    /// Create a new ReAct strategy.
    pub fn new(
        prompt_registry: Arc<PromptRegistry>,
        model: Arc<ConnectionManager>,
        tools: ToolRegistry,
        memory: Arc<Mutex<M>>,
        max_iterations: usize,
    ) -> Self {
        Self {
            skeleton: CycleSkeleton::new(prompt_registry, model, tools, memory, max_iterations),
            pending_answer: None,
        }
    }

    /// Execute the ReAct loop.
    ///
    /// The loop runs until the model returns an answer, an error occurs,
    /// or max iterations is reached.
    pub async fn run(&mut self, input: String) -> Result<agentverse::CycleResult, AgentError> {
        // Insert the react preamble as the first message when a react.j2 file
        // Reset per-run state.
        self.skeleton.reset_iteration();
        self.pending_answer = None;

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

            let iteration = self.skeleton.current_iteration();
            match action {
                CycleAction::Continue { thought } => {
                    // If we already nudged last iteration and the model still didn't
                    // emit Answer: or Action:, the pre-nudge thought was already a
                    // complete answer — use it rather than the confused post-nudge one.
                    if let Some(saved) = self.pending_answer.take() {
                        info!(
                            iteration,
                            action = "done",
                            total_input_tokens = self.skeleton.total_usage().input_tokens,
                            total_output_tokens = self.skeleton.total_usage().output_tokens,
                            "Strategy completed (nudge fallback)"
                        );
                        return Ok(agentverse::CycleResult {
                            answer: saved,
                            total_usage: self.skeleton.total_usage(),
                        });
                    }

                    info!(iteration, action = "continue", "Thought only, continuing");
                    self.pending_answer = Some(thought.clone());
                    let mut mem = self.skeleton.memory().lock().await;
                    mem.append(agentverse::Message {
                        role: agentverse::memory::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                    // Anthropic rejects requests ending with an assistant message.
                    // The nudge pushes the model toward a terminal action.
                    mem.append(agentverse::Message {
                        role: agentverse::memory::MessageRole::User,
                        content: "Either call a tool (Action: / Action Input:) or give your final answer (Answer: ...).".to_string(),
                    });
                }
                CycleAction::ToolCall { tool_name, args } => {
                    self.pending_answer = None;
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
                    info!(iteration, action = "tool_call", tool = %tool_name, "Tool executed");
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
                    info!(
                        iteration,
                        action = "done",
                        total_input_tokens = self.skeleton.total_usage().input_tokens,
                        total_output_tokens = self.skeleton.total_usage().output_tokens,
                        "Strategy completed"
                    );
                    return Ok(agentverse::CycleResult {
                        answer,
                        total_usage: self.skeleton.total_usage(),
                    });
                }
                CycleAction::Error { message } => {
                    tracing::error!(iteration, error = %message, "Strategy error");
                    return Err(AgentError::Model(agentverse::ModelError::InvalidResponse(
                        message,
                    )));
                }
            }
        }
    }

    /// Return a reference to the underlying cycle skeleton.
    pub fn skeleton(&self) -> &CycleSkeleton<M> {
        &self.skeleton
    }
}

#[async_trait::async_trait]
impl<M> agentverse::RunStrategy for ReActStrategy<M>
where
    M: agentverse::Memory + Send + 'static,
{
    async fn process(&mut self, input: String) -> Result<String, agentverse::AgentError> {
        self.run(input).await.map(|r| r.answer)
    }
}
