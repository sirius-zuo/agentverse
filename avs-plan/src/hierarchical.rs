//! Hierarchical Planning strategy.
//!
//! Decomposes complex requests into sub-goals, then generates and executes
//! a plan for each sub-goal, and finally synthesizes results from all sub-goals.

use super::planner::{decompose_request, generate_plan};
use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, LlmRunner, PromptRegistry};
use agentverse_guardrails::check_output;
use agentverse_tools::ToolRegistry;
use std::sync::Arc;

/// Hierarchical Planning strategy.
pub struct HierarchicalStrategy {
    runner: Arc<LlmRunner>,
    prompts: Arc<PromptRegistry>,
    tools: Arc<ToolRegistry>,
    max_iterations: usize,
    max_decompose_depth: usize,
}

impl HierarchicalStrategy {
    /// Create a new Hierarchical Planning strategy.
    pub fn new(
        runner: Arc<LlmRunner>,
        prompts: Arc<PromptRegistry>,
        tools: Arc<ToolRegistry>,
        max_iterations: usize,
        max_decompose_depth: usize,
    ) -> Self {
        Self {
            runner,
            prompts,
            tools,
            max_iterations,
            max_decompose_depth,
        }
    }

    /// Execute a single tool by name.
    async fn execute_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<String, AgentError> {
        let result = self
            .tools
            .execute(tool_name, args)
            .await
            .map_err(AgentError::Tool)?;
        Ok(match result {
            serde_json::Value::String(s) => s,
            v => v.to_string(),
        })
    }
}

#[async_trait::async_trait]
impl agentverse::RunStrategy for HierarchicalStrategy {
    async fn run(&self, messages: Vec<Message>) -> Result<agentverse::StrategyOutcome, AgentError> {
        let all_names = self.tools.tool_names();
        self.run_with_active_tools(messages, &all_names).await
    }

    async fn run_with_active_tools(
        &self,
        messages: Vec<Message>,
        active_tool_names: &[String],
    ) -> Result<agentverse::StrategyOutcome, AgentError> {
        let input = messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, MessageRole::User))
            .map(|m| m.content.clone())
            .unwrap_or_default();

        let sub_goals = decompose_request(&self.runner, &self.prompts, &input).await?;

        let mut sub_goal_results: Vec<(usize, String)> = Vec::new();

        for (i, sub_goal) in sub_goals.iter().enumerate() {
            if i >= self.max_decompose_depth {
                break;
            }

            let tool_summaries = self.tools.tool_summaries_for(active_tool_names);

            let conversation = messages
                .iter()
                .map(|m| {
                    let role = match m.role {
                        MessageRole::System => "System",
                        MessageRole::User => "User",
                        MessageRole::Assistant => "Assistant",
                        MessageRole::Tool => "Tool",
                    };
                    format!("{}: {}", role, m.content)
                })
                .collect::<Vec<_>>()
                .join("\n");

            let sub_plan = match generate_plan(
                &self.runner,
                &self.prompts,
                sub_goal,
                &tool_summaries,
                &conversation,
            )
            .await
            {
                Ok(p) => p,
                Err(e) => {
                    sub_goal_results.push((i, format!("Plan generation failed: {e}")));
                    continue;
                }
            };

            let mut step_results: Vec<String> = Vec::new();
            for step in &sub_plan.steps {
                if step.id > self.max_iterations {
                    break;
                }

                let result = if let Some(ref tool_name) = step.tool {
                    if !active_tool_names.is_empty() && !active_tool_names.contains(tool_name) {
                        format!("Tool '{}' is not available for this session", tool_name)
                    } else {
                        let args = step.args.clone().unwrap_or_default();
                        match self.execute_tool(tool_name, args).await {
                            Ok(r) => r,
                            Err(e) => format!("Tool error: {}", e),
                        }
                    }
                } else {
                    format!("Reasoning: {}", step.description)
                };

                step_results.push(result);
            }

            sub_goal_results.push((i, step_results.join("\n")));
        }

        let final_prompt = format!(
            "All sub-goals have been executed. Provide a comprehensive answer to the user's request.\n\n\
             User request: {}\n\n\
             Sub-goal results:\n{}",
            input,
            sub_goal_results
                .iter()
                .map(|(id, r)| format!("Sub-goal {}: {}", id, r))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let synthesis_messages = vec![
            Message {
                role: MessageRole::System,
                content: final_prompt,
            },
            Message {
                role: MessageRole::User,
                content: "Based on the completed sub-goals, provide the final answer.".to_string(),
            },
        ];

        let answer = self.runner.invoke(synthesis_messages).await?;

        check_output(&answer.content).map_err(|e| {
            AgentError::Guardrail(match e {
                agentverse_guardrails::GuardrailError::OutputFiltered(msg) => {
                    agentverse::GuardrailError::OutputFiltered(msg)
                }
                agentverse_guardrails::GuardrailError::PromptInjection(msg) => {
                    agentverse::GuardrailError::PromptInjection(msg)
                }
                _ => agentverse::GuardrailError::OutputFiltered(e.to_string()),
            })
        })?;

        Ok(agentverse::StrategyOutcome::Done(answer.content))
    }
}
