//! ReAct strategy implementation.
//!
//! Implements the ReAct pattern: Think → Act → Observe → Think...
//! Uses the CycleSkeleton for utility methods and the shared cycle loop.

use super::cycle::{CycleAction, CycleSkeleton};
use super::parse::parse_response;
use agentverse::{AgentError, LlmRunner, Message, ModelError, PromptRegistry, ToolCall};
use agentverse_tools::{ActiveToolSet, ToolRegistry};
use std::fmt::Write;
use std::sync::Arc;
use tracing::info;

fn base64_encode(s: &str) -> String {
    // Simple percent-encoding to avoid colon conflicts in the HITL: message format
    let mut out = String::new();
    for b in s.bytes() {
        if b == b':' || b == b'%' {
            let _ = write!(out, "%{:02X}", b);
        } else {
            out.push(b as char);
        }
    }
    out
}

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

    fn prepare_buffer_with_active(
        &self,
        messages: Vec<Message>,
        active: &ActiveToolSet,
    ) -> Vec<Message> {
        if !self.skeleton.prompts.has_react_template() {
            return messages;
        }
        let tools_str = self.skeleton.build_tools_str_active(active);
        let mut ctx = std::collections::HashMap::new();
        ctx.insert("tools".to_string(), serde_json::Value::String(tools_str));
        if let Some(examples) = self.skeleton.prompts.get_examples("react_examples") {
            if let Ok(val) = serde_json::to_value(examples) {
                ctx.insert("examples".to_string(), val);
            }
        }
        let mut buf = messages;
        if let Ok(preamble) = self.skeleton.prompts.render("react", ctx) {
            if !preamble.trim().is_empty() {
                let insert_pos = buf
                    .iter()
                    .position(|m| !matches!(m.role, agentverse::MessageRole::System))
                    .unwrap_or(0);
                buf.insert(
                    insert_pos,
                    Message {
                        role: agentverse::MessageRole::User,
                        content: preamble,
                    },
                );
            }
        }
        buf
    }
}

#[async_trait::async_trait]
impl agentverse::RunStrategy for ReActStrategy {
    async fn run(&self, messages: Vec<Message>) -> Result<String, AgentError> {
        let all_names = self.skeleton.tools.tool_names();
        self.run_with_active_tools(messages, &all_names).await
    }

    async fn run_with_active_tools(
        &self,
        messages: Vec<Message>,
        active_tool_names: &[String],
    ) -> Result<String, AgentError> {
        let mut active = ActiveToolSet::default();
        active.activate(
            &active_tool_names
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        );
        let mut buf = self.prepare_buffer_with_active(messages, &active);
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
                    buf.push(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                    pending_answer = Some(thought);
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
                CycleAction::ToolCalls { calls } => {
                    pending_answer = None;
                    buf.push(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: response.content.clone(),
                    });
                    let results = self.skeleton.execute_many(calls).await?;
                    info!(
                        iteration,
                        action = "tool_calls",
                        count = results.len(),
                        "Parallel tools executed"
                    );
                    let observation = results
                        .iter()
                        .map(|r| {
                            let v = match &r.result {
                                Ok(v) => v.to_string(),
                                Err(e) => format!("Error: {e}"),
                            };
                            format!("Tool: {}\nResult: {}", r.name, v)
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    buf.push(Message {
                        role: agentverse::MessageRole::User,
                        content: observation,
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

    async fn run_hitl(
        &self,
        messages: Vec<Message>,
        active_tool_names: &[String],
        hook: Arc<dyn agentverse::hitl::HitlHook>,
    ) -> Result<String, AgentError> {
        let mut active = ActiveToolSet::default();
        active.activate(&active_tool_names.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let mut buf = self.prepare_buffer_with_active(messages, &active);
        let mut iteration = 0usize;
        let mut pending_answer: Option<String> = None;

        loop {
            if iteration >= self.skeleton.max_iterations() {
                return Err(AgentError::Model(ModelError::Timeout(format!(
                    "Max iterations ({}) reached", self.skeleton.max_iterations()
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
                    buf.push(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                    pending_answer = Some(thought);
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
                    let calls = vec![ToolCall {
                        name: tool_name.clone(),
                        args: args.clone(),
                    }];
                    match self.skeleton.tools.execute_many_hitl(calls, &hook).await {
                        Ok(results) => {
                            let r = &results[0];
                            let v = match &r.result {
                                Ok(v) => v.to_string(),
                                Err(e) => format!("Error: {e}"),
                            };
                            buf.push(Message {
                                role: agentverse::MessageRole::User,
                                content: format!("Tool: {}\nResult: {}", tool_name, v),
                            });
                        }
                        Err(AgentError::Interrupted(approval_id)) => {
                            let history_json = serde_json::to_string(&buf).unwrap_or_default();
                            let pending_json = serde_json::to_string(&[serde_json::json!({
                                "name": tool_name,
                                "args": args,
                            })]).unwrap_or_default();
                            return Err(AgentError::Memory(format!(
                                "HITL:{}:{}:{}",
                                approval_id,
                                base64_encode(&history_json),
                                base64_encode(&pending_json),
                            )));
                        }
                        Err(e) => return Err(e),
                    }
                }
                CycleAction::ToolCalls { calls } => {
                    pending_answer = None;
                    buf.push(Message {
                        role: agentverse::MessageRole::Assistant,
                        content: response.content.clone(),
                    });
                    match self.skeleton.tools.execute_many_hitl(calls.clone(), &hook).await {
                        Ok(results) => {
                            let observation = results.iter().map(|r| {
                                let v = match &r.result {
                                    Ok(v) => v.to_string(),
                                    Err(e) => format!("Error: {e}"),
                                };
                                format!("Tool: {}\nResult: {}", r.name, v)
                            }).collect::<Vec<_>>().join("\n\n");
                            buf.push(Message {
                                role: agentverse::MessageRole::User,
                                content: observation,
                            });
                        }
                        Err(AgentError::Interrupted(approval_id)) => {
                            let history_json = serde_json::to_string(&buf).unwrap_or_default();
                            let pending_json = serde_json::Value::Array(
                                calls.iter().map(|c| {
                                    serde_json::json!({"name": c.name, "args": c.args})
                                }).collect()
                            );
                            let pending_json = serde_json::to_string(&pending_json).unwrap_or_default();
                            return Err(AgentError::Memory(format!(
                                "HITL:{}:{}:{}",
                                approval_id,
                                base64_encode(&history_json),
                                base64_encode(&pending_json),
                            )));
                        }
                        Err(e) => return Err(e),
                    }
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
