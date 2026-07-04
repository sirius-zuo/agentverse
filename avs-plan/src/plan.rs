//! Plan-and-Execute strategy.
//!
//! Generates a plan from the request, then executes each step sequentially,
//! and finally synthesizes a result from all step outputs.

use super::planner::generate_plan;
use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, LlmRunner, PromptRegistry};
use agentverse_guardrails::check_output;
use agentverse_tools::ToolRegistry;
use std::sync::Arc;

/// Plan-and-Execute strategy: plan first, then execute.
///
/// 1. Generate a plan from the user's request
/// 2. Execute each step sequentially (tools or reasoning)
/// 3. Ask the model to synthesize a final answer from all results
pub struct PlanStrategy {
    runner: Arc<LlmRunner>,
    prompts: Arc<PromptRegistry>,
    tools: Arc<ToolRegistry>,
    max_iterations: usize,
}

impl PlanStrategy {
    /// Create a new Plan-and-Execute strategy.
    pub fn new(
        runner: Arc<LlmRunner>,
        prompts: Arc<PromptRegistry>,
        tools: Arc<ToolRegistry>,
        max_iterations: usize,
    ) -> Self {
        Self {
            runner,
            prompts,
            tools,
            max_iterations,
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
impl agentverse::RunStrategy for PlanStrategy {
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

        let plan = generate_plan(
            &self.runner,
            &self.prompts,
            &input,
            &tool_summaries,
            &conversation,
        )
        .await?;

        let mut step_results: Vec<(usize, String)> = Vec::new();

        for step in &plan.steps {
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

            step_results.push((step.id, result));
        }

        let final_prompt = format!(
            "You executed the following plan:\nPlan: {}\n\nStep results:\n{}\n\nProvide the final answer to: {}",
            plan.description,
            step_results
                .iter()
                .map(|(id, r)| format!("Step {}: {}", id, r))
                .collect::<Vec<_>>()
                .join("\n"),
            input
        );

        let synthesis_messages = vec![
            Message {
                role: MessageRole::System,
                content: final_prompt,
            },
            Message {
                role: MessageRole::User,
                content: "Based on the executed plan, provide the final answer.".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry, RunStrategy};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

    fn make_plan_strategy() -> PlanStrategy {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::OpenAI {
                    model_name: "test".to_string(),
                    api_key: "sk-test".to_string(),
                    base_url: Some("http://127.0.0.1:1/v1".to_string()),
                },
                max_messages: 10,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap(),
        );
        PlanStrategy::new(
            runner,
            Arc::new(PromptRegistry::new()),
            ToolRegistry::new(),
            5,
        )
    }

    #[tokio::test]
    async fn plan_run_returns_error_on_bad_port() {
        let strategy = make_plan_strategy();
        let messages = vec![agentverse::Message {
            role: agentverse::memory::MessageRole::User,
            content: "Search for rust".to_string(),
        }];
        let result = strategy.run(messages).await;
        assert!(result.is_err());
    }
}
