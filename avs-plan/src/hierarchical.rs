//! Hierarchical Planning strategy.
//!
//! Decomposes complex requests into sub-goals, then generates and executes
//! a plan for each sub-goal, and finally synthesizes results from all sub-goals.

use super::planner::{decompose_request, generate_plan, Plan};
use agentverse::{AgentError, ModelProvider, PromptRegistry, SyncTool};
use agentverse_guardrails::check_output;
use std::sync::{Arc, Mutex};

/// Hierarchical Planning strategy.
pub struct HierarchicalStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    model: Arc<P>,
    registry: Arc<PromptRegistry>,
    tools: Vec<Box<dyn SyncTool>>,
    memory: Arc<Mutex<M>>,
    max_iterations: usize,
    max_decompose_depth: usize,
}

impl<P, M> HierarchicalStrategy<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    /// Create a new Hierarchical Planning strategy.
    pub fn new(
        model: Arc<P>,
        registry: Arc<PromptRegistry>,
        tools: Vec<Box<dyn SyncTool>>,
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
        self.memory.lock().unwrap().append(agentverse::Message {
            role: agentverse::memory::MessageRole::User,
            content: input.clone(),
        });

        let sub_goals = decompose_request(&*self.model, &self.registry, &input).await?;

        self.memory.lock().unwrap().append(agentverse::Message {
            role: agentverse::memory::MessageRole::System,
            content: format!("Decomposed into {} sub-goals", sub_goals.len()),
        });

        let mut sub_goal_results: Vec<(usize, String)> = Vec::new();

        for (i, sub_goal) in sub_goals.iter().enumerate() {
            if i >= self.max_decompose_depth {
                self.memory.lock().unwrap().append(agentverse::Message {
                    role: agentverse::memory::MessageRole::System,
                    content: format!(
                        "Stopping sub-goal decomposition at depth {}: max depth ({}) reached",
                        i + 1,
                        self.max_decompose_depth
                    ),
                });
                break;
            }

            let tool_names: Vec<String> = self.tools.iter().map(|t| t.name().to_string()).collect();

            let conversation = self
                .memory
                .lock()
                .unwrap()
                .last_n(20)
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

            let sub_plan = generate_plan(&*self.model, &self.registry, sub_goal, &tool_names, &conversation).await?;

            let mut step_results: Vec<String> = Vec::new();
            for step in &sub_plan.steps {
                if step.id > self.max_iterations {
                    self.memory.lock().unwrap().append(agentverse::Message {
                        role: agentverse::memory::MessageRole::System,
                        content: format!("Sub-goal {} step {}: max iterations reached", i, step.id),
                    });
                    break;
                }

                let result = if let Some(ref tool_name) = step.tool {
                    let args = step.args.clone().unwrap_or_default();
                    match self.execute_tool(tool_name, args) {
                        Ok(result) => result,
                        Err(e) => format!("Tool error: {}", e),
                    }
                } else {
                    format!("Reasoning: {}", step.description)
                };

                step_results.push(result.clone());

                self.memory.lock().unwrap().append(agentverse::Message {
                    role: agentverse::memory::MessageRole::System,
                    content: format!(
                        "Sub-goal {} step {} ({}): {}",
                        i,
                        step.id,
                        step.tool.as_deref().unwrap_or("reasoning"),
                        result
                    ),
                });
            }

            let sub_result = step_results.join("\n");
            sub_goal_results.push((i, sub_result.clone()));

            self.memory.lock().unwrap().append(agentverse::Message {
                role: agentverse::memory::MessageRole::System,
                content: format!("Sub-goal {} completed: {}", i, sub_result),
            });
        }

        let conversation_history = self
            .memory
            .lock()
            .unwrap()
            .last_n(30)
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

        let answer = self
            .model
            .generate(&final_prompt, None)
            .await
            .map_err(AgentError::Model)?;

        check_output(&answer).map_err(|e| AgentError::Guardrail(match e {
            agentverse_guardrails::GuardrailError::OutputFiltered(msg) => agentverse::GuardrailError::OutputFiltered(msg),
            agentverse_guardrails::GuardrailError::PromptInjection(msg) => agentverse::GuardrailError::PromptInjection(msg),
            _ => agentverse::GuardrailError::OutputFiltered(e.to_string()),
        }))?;

        Ok(answer)
    }

    /// Execute a single tool by name.
    fn execute_tool(&self, tool_name: &str, args: serde_json::Value) -> Result<String, AgentError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| {
                AgentError::Tool(agentverse::ToolError::NotFound(tool_name.to_string()))
            })?;

        let result = tool.execute(args).map_err(AgentError::Tool)?;
        Ok(result.to_string())
    }
}
