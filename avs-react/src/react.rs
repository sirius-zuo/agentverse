//! ReAct strategy implementation.
//!
//! Implements the ReAct pattern using each provider's native structured tool
//! calling: the model's response *is* the assistant turn (`Text` and/or
//! `ToolUse` blocks), dispatched directly with no free-text parsing.

use super::cycle::{
    action_from_response, reconcile_and_order_results, results_to_tool_result_blocks, CycleAction,
    CycleSkeleton,
};
use agentverse::{
    AgentError, ConfigError, LlmRunner, Message, MessageRole, ModelError, PromptRegistry,
    StrategyOutcome,
};
use agentverse_tools::{HitlInterruptResult, ToolRegistry};
use std::sync::Arc;
use tracing::info;

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

    async fn invoke_with_active_tools(
        &self,
        messages: Vec<Message>,
        active_tool_names: &[String],
    ) -> Result<agentverse::GenerateResponse, AgentError> {
        let definitions = self
            .skeleton
            .tools
            .tool_definitions_for(active_tool_names)
            .map_err(|e| {
                AgentError::Config(ConfigError::Invalid(format!(
                    "invalid tool schema for active tools: {e}"
                )))
            })?;
        if definitions.is_empty() {
            self.skeleton.runner.invoke(messages).await
        } else {
            self.skeleton
                .runner
                .invoke_with_tools(messages, definitions)
                .await
        }
    }
}

#[async_trait::async_trait]
impl agentverse::RunStrategy for ReActStrategy {
    async fn run(&self, messages: Vec<Message>) -> Result<StrategyOutcome, AgentError> {
        let all_names = self.skeleton.tools.tool_names();
        self.run_with_active_tools(messages, &all_names).await
    }

    async fn run_with_active_tools(
        &self,
        messages: Vec<Message>,
        active_tool_names: &[String],
    ) -> Result<StrategyOutcome, AgentError> {
        let mut buf = self.skeleton.prepare_buffer(messages);
        let mut iteration = 0usize;

        loop {
            if iteration >= self.skeleton.max_iterations() {
                return Err(AgentError::Model(ModelError::Timeout(format!(
                    "Max iterations ({}) reached",
                    self.skeleton.max_iterations()
                ))));
            }
            iteration += 1;

            let response = self
                .invoke_with_active_tools(buf.clone(), active_tool_names)
                .await?;
            self.skeleton.check_output_guardrail(&response.as_text())?;

            match action_from_response(&response) {
                CycleAction::ToolCalls { calls } => {
                    buf.push(Message {
                        role: MessageRole::Assistant,
                        content: response.content.clone(),
                    });
                    let order: Vec<(String, String)> = calls
                        .iter()
                        .map(|c| (c.id.clone(), c.name.clone()))
                        .collect();
                    let results = self.skeleton.execute_many(calls).await?;
                    let results = reconcile_and_order_results(results, &order);
                    info!(
                        iteration,
                        action = "tool_calls",
                        count = results.len(),
                        "Tools executed"
                    );
                    buf.push(Message {
                        role: MessageRole::Tool,
                        content: results_to_tool_result_blocks(results),
                    });
                }
                CycleAction::Done { answer } => {
                    info!(iteration, "Strategy completed");
                    return Ok(StrategyOutcome::Done(answer));
                }
                CycleAction::Error { message } => {
                    tracing::error!(iteration, error = %message, "Strategy error");
                    return Err(AgentError::Model(ModelError::InvalidResponse(message)));
                }
            }
        }
    }

    async fn run_hitl(
        &self,
        messages: Vec<Message>,
        active_tool_names: &[String],
        hook: Arc<dyn agentverse::hitl::HitlHook>,
    ) -> Result<StrategyOutcome, AgentError> {
        let mut buf = self.skeleton.prepare_buffer(messages);
        let mut iteration = 0usize;

        // Tools the skill declares as mandatory (hitl_tools / checkpoints):
        // the model must call at least one of these before the invocation
        // is allowed to finish. Empty means nothing is mandatory here.
        let required_tools = hook.required_tool_names();
        let mut required_tool_called = required_tools.is_empty();

        loop {
            if iteration >= self.skeleton.max_iterations() {
                return Err(AgentError::Model(ModelError::Timeout(format!(
                    "Max iterations ({}) reached",
                    self.skeleton.max_iterations()
                ))));
            }
            iteration += 1;

            let response = self
                .invoke_with_active_tools(buf.clone(), active_tool_names)
                .await?;
            self.skeleton.check_output_guardrail(&response.as_text())?;

            match action_from_response(&response) {
                CycleAction::ToolCalls { calls } => {
                    if !required_tool_called {
                        required_tool_called =
                            calls.iter().any(|c| required_tools.contains(&c.name));
                    }
                    // Snapshot BEFORE pushing the assistant message so history
                    // on suspend does not contain a dangling tool-call turn
                    // with no observation.
                    let history_snapshot = buf.clone();
                    buf.push(Message {
                        role: MessageRole::Assistant,
                        content: response.content.clone(),
                    });
                    let order: Vec<(String, String)> = calls
                        .iter()
                        .map(|c| (c.id.clone(), c.name.clone()))
                        .collect();
                    match self
                        .skeleton
                        .tools
                        .execute_many_hitl(calls.clone(), &hook)
                        .await
                    {
                        Ok(results) => {
                            let results = reconcile_and_order_results(results, &order);
                            info!(
                                iteration,
                                action = "tool_calls",
                                count = results.len(),
                                "Tools executed"
                            );
                            buf.push(Message {
                                role: MessageRole::Tool,
                                content: results_to_tool_result_blocks(results),
                            });
                        }
                        Err(HitlInterruptResult {
                            approval_id,
                            kind_json,
                        }) => {
                            return Ok(StrategyOutcome::Interrupted(
                                agentverse::hitl::HitlInterrupt {
                                    approval_id,
                                    kind_json,
                                    history: history_snapshot,
                                    pending_calls: calls,
                                    active_tool_names: active_tool_names.to_vec(),
                                },
                            ));
                        }
                    }
                }
                CycleAction::Done { answer } => {
                    if !required_tool_called {
                        tracing::warn!(
                            iteration,
                            required = ?required_tools,
                            "Model tried to finish without calling a required HITL tool; forcing retry"
                        );
                        buf.push(Message {
                            role: MessageRole::Assistant,
                            content: response.content.clone(),
                        });
                        buf.push(Message::text(
                            MessageRole::User,
                            format!(
                                "You did not call the required tool ({}) before answering. \
                                 This step requires human approval — you must call it before \
                                 you can finish. Call it now.",
                                required_tools.join(", ")
                            ),
                        ));
                        continue;
                    }
                    info!(iteration, "Strategy completed");
                    return Ok(StrategyOutcome::Done(answer));
                }
                CycleAction::Error { message } => {
                    tracing::error!(iteration, error = %message, "Strategy error");
                    return Err(AgentError::Model(ModelError::InvalidResponse(message)));
                }
            }
        }
    }
}
