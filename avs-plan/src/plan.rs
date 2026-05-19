//! Plan-and-Execute strategy.
//!
//! Generates a plan from the request, then executes each step sequentially,
//! and finally synthesizes a result from all step outputs.

use super::planner::generate_plan;
use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, GenerateRequest, ModelProvider, PromptRegistry};
use agentverse_guardrails::check_output;
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Plan-and-Execute strategy: plan first, then execute.
///
/// 1. Generate a plan from the user's request
/// 2. Execute each step sequentially (tools or reasoning)
/// 3. Ask the model to synthesize a final answer from all results
pub struct PlanStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    model: Arc<P>,
    registry: Arc<PromptRegistry>,
    tools: ToolRegistry,
    memory: Arc<Mutex<M>>,
    max_iterations: usize,
}

impl<P, M> PlanStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    /// Create a new Plan-and-Execute strategy.
    pub fn new(
        model: Arc<P>,
        registry: Arc<PromptRegistry>,
        tools: ToolRegistry,
        memory: Arc<Mutex<M>>,
        max_iterations: usize,
    ) -> Self {
        Self {
            model,
            registry,
            tools,
            memory,
            max_iterations,
        }
    }

    /// Execute the plan-and-execute cycle.
    pub async fn run(&mut self, input: String) -> Result<String, AgentError> {
        self.memory.lock().await.append(agentverse::Message {
            role: agentverse::memory::MessageRole::User,
            content: input.clone(),
        });

        let tool_names: Vec<String> = self.tools.tool_names();

        let conversation = self
            .memory
            .lock()
            .await
            .last_n(20)
            .await
            .map_err(|e| AgentError::Memory(e.to_string()))?
            .iter()
            .map(|m| {
                let role_str = match m.role {
                    agentverse::memory::MessageRole::System => "System",
                    agentverse::memory::MessageRole::User => "User",
                    agentverse::memory::MessageRole::Assistant => "Assistant",
                    agentverse::memory::MessageRole::Tool => "Tool",
                };
                format!("{}: {}", role_str, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let plan = generate_plan(
            &*self.model,
            &self.registry,
            &input,
            &tool_names,
            &conversation,
        )
        .await?;

        self.memory.lock().await.append(agentverse::Message {
            role: agentverse::memory::MessageRole::System,
            content: format!("Plan generated: {}", plan.description),
        });

        let mut step_results: Vec<(usize, String)> = Vec::new();

        for step in &plan.steps {
            if step.id > self.max_iterations {
                self.memory.lock().await.append(agentverse::Message {
                    role: agentverse::memory::MessageRole::System,
                    content: format!(
                        "Stopping at step {}: max iterations ({}) reached",
                        step.id, self.max_iterations
                    ),
                });
                break;
            }

            let result = if let Some(ref tool_name) = step.tool {
                let args = step.args.clone().unwrap_or_default();
                match self.execute_tool(tool_name, args).await {
                    Ok(result) => result,
                    Err(e) => format!("Tool error: {}", e),
                }
            } else {
                format!("Reasoning: {}", step.description)
            };

            step_results.push((step.id, result.clone()));

            self.memory.lock().await.append(agentverse::Message {
                role: agentverse::memory::MessageRole::System,
                content: format!("Step {} executed: {}", step.id, result),
            });
        }

        // Phase 3: Synthesize final answer
        let conversation_history = self
            .memory
            .lock()
            .await
            .last_n(20)
            .await
            .map_err(|e| AgentError::Memory(e.to_string()))?
            .iter()
            .map(|m| {
                let role_str = match m.role {
                    agentverse::memory::MessageRole::System => "System",
                    agentverse::memory::MessageRole::User => "User",
                    agentverse::memory::MessageRole::Assistant => "Assistant",
                    agentverse::memory::MessageRole::Tool => "Tool",
                };
                format!("{}: {}", role_str, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let final_prompt = format!(
            "You executed the following plan:\n\
             Plan: {}\n\n\
             Step results:\n{}\n\n\
             Based on these results, provide the final answer to the user's request.\n\n\
             User request: {}\n\n\
             Conversation history:\n{}",
            plan.description,
            step_results
                .iter()
                .map(|(id, result)| format!("Step {}: {}", id, result))
                .collect::<Vec<_>>()
                .join("\n"),
            input,
            conversation_history
        );

        let gen_request = GenerateRequest {
            system: Some(final_prompt),
            messages: vec![Message {
                role: MessageRole::User,
                content: "Based on the executed plan, provide the final answer.".to_string(),
            }],
            tools: None,
        };

        let answer = self
            .model
            .generate(gen_request)
            .await
            .map_err(AgentError::Model)?;

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

        Ok(answer.content)
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
        Ok(result.to_string())
    }
}
