//! Hierarchical Planning strategy.
//!
//! Decomposes complex requests into sub-goals, then generates and executes
//! a plan for each sub-goal, and finally synthesizes results from all sub-goals.

use super::planner::{decompose_request, generate_plan};
use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, GenerateRequest, PromptRegistry, ProviderWrapper};
use agentverse_guardrails::check_output;
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Hierarchical Planning strategy.
pub struct HierarchicalStrategy<M>
where
    M: agentverse::Memory,
{
    model: Arc<ProviderWrapper>,
    registry: Arc<PromptRegistry>,
    tools: ToolRegistry,
    memory: Arc<Mutex<M>>,
    max_iterations: usize,
    max_decompose_depth: usize,
}

impl<M> HierarchicalStrategy<M>
where
    M: agentverse::Memory,
{
    /// Create a new Hierarchical Planning strategy.
    pub fn new(
        model: Arc<ProviderWrapper>,
        registry: Arc<PromptRegistry>,
        tools: ToolRegistry,
        memory: Arc<Mutex<M>>,
        max_iterations: usize,
        max_decompose_depth: usize,
    ) -> Self {
        Self {
            model,
            registry,
            tools,
            memory,
            max_iterations,
            max_decompose_depth,
        }
    }

    /// Execute the hierarchical planning cycle.
    pub async fn run(&mut self, input: String) -> Result<String, AgentError> {
        self.memory.lock().await.append(agentverse::Message {
            role: agentverse::memory::MessageRole::User,
            content: input.clone(),
        });

        let sub_goals = decompose_request(&*self.model, &self.registry, &input).await?;

        self.memory.lock().await.append(agentverse::Message {
            role: agentverse::memory::MessageRole::System,
            content: format!("Decomposed into {} sub-goals", sub_goals.len()),
        });

        let mut sub_goal_results: Vec<(usize, String)> = Vec::new();

        for (i, sub_goal) in sub_goals.iter().enumerate() {
            if i >= self.max_decompose_depth {
                self.memory.lock().await.append(agentverse::Message {
                    role: agentverse::memory::MessageRole::System,
                    content: format!(
                        "Stopping sub-goal decomposition at depth {}: max depth ({}) reached",
                        i + 1,
                        self.max_decompose_depth
                    ),
                });
                break;
            }

            let tool_summaries = self.tools.tool_summaries();

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

            let sub_plan = match generate_plan(
                &*self.model,
                &self.registry,
                sub_goal,
                &tool_summaries,
                &conversation,
            )
            .await
            {
                Ok(p) => p,
                Err(e) => {
                    let msg = format!("Plan generation failed: {e}");
                    sub_goal_results.push((i, msg.clone()));
                    self.memory.lock().await.append(agentverse::Message {
                        role: agentverse::memory::MessageRole::System,
                        content: format!("Sub-goal {i}: {msg}"),
                    });
                    continue;
                }
            };

            let mut step_results: Vec<String> = Vec::new();
            for step in &sub_plan.steps {
                if step.id > self.max_iterations {
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

                step_results.push(result);
            }

            let sub_result = step_results.join("\n");
            sub_goal_results.push((i, sub_result.clone()));

            self.memory.lock().await.append(agentverse::Message {
                role: agentverse::memory::MessageRole::System,
                content: format!("Sub-goal {}: {}", i, sub_result),
            });
        }

        let conversation_history = self
            .memory
            .lock()
            .await
            .last_n(30)
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
            "All sub-goals have been executed. Provide a comprehensive answer to the user's request.\n\n\
             User request: {}\n\n\
             Sub-goal results:\n{}\n\n\
             Conversation history:\n{}",
            input,
            sub_goal_results
                .iter()
                .map(|(id, result)| format!("Sub-goal {}: {}", id, result))
                .collect::<Vec<_>>()
                .join("\n"),
            conversation_history
        );

        let gen_request = GenerateRequest {
            system: Some(final_prompt),
            messages: vec![Message {
                role: MessageRole::User,
                content: "Based on the completed sub-goals, provide the final answer.".to_string(),
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
        Ok(match result {
            serde_json::Value::String(s) => s,
            v => v.to_string(),
        })
    }
}

#[async_trait::async_trait]
impl<M> agentverse::RunStrategy for HierarchicalStrategy<M>
where
    M: agentverse::Memory + Send + 'static,
{
    async fn process(&mut self, input: String) -> Result<String, agentverse::AgentError> {
        self.run(input).await
    }
}
