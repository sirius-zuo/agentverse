//! Plan-and-Execute strategy.
//!
//! Generates a plan from the request, then executes each step sequentially,
//! and finally synthesizes a result from all step outputs.

use super::planner::generate_plan;
use agentverse::{AgentError, ModelProvider, SyncTool};
use std::sync::{Arc, Mutex};

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
    tools: Vec<Box<dyn SyncTool>>,
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
        tools: Vec<Box<dyn SyncTool>>,
        memory: Arc<Mutex<M>>,
        max_iterations: usize,
    ) -> Self {
        Self {
            model,
            tools,
            memory,
            max_iterations,
        }
    }

    /// Execute the plan-and-execute cycle.
    ///
    /// # Arguments
    /// * `input` - The user's request
    ///
    /// # Returns
    /// A string containing the final synthesized answer
    pub async fn run(&mut self, input: String) -> Result<String, AgentError> {
        // Phase 1: Generate plan
        let tool_names: Vec<String> = self
            .tools
            .iter()
            .map(|t| t.name().to_string())
            .collect();

        let plan = generate_plan(&*self.model, &input, &tool_names).await?;

        self.memory.lock().unwrap().append(agentverse::Message {
            role: agentverse::memory::MessageRole::System,
            content: format!("Plan generated: {}", plan.description),
        });

        // Phase 2: Execute each step sequentially
        let mut step_results: Vec<(usize, String)> = Vec::new();

        for step in &plan.steps {
            // Check if we've exceeded max iterations
            if step.id > self.max_iterations {
                self.memory.lock().unwrap().append(agentverse::Message {
                    role: agentverse::memory::MessageRole::System,
                    content: format!(
                        "Stopping at step {}: max iterations ({}) reached",
                        step.id, self.max_iterations
                    ),
                });
                break;
            }

            let result = if let Some(ref tool_name) = step.tool {
                // Execute a tool
                let args = step.args.clone().unwrap_or_default();
                match self.execute_tool(tool_name, args) {
                    Ok(result) => result,
                    Err(e) => format!("Tool error: {}", e),
                }
            } else {
                // Reasoning-only step: just acknowledge
                format!("Reasoning: {}", step.description)
            };

            step_results.push((step.id, result.clone()));

            self.memory.lock().unwrap().append(agentverse::Message {
                role: agentverse::memory::MessageRole::System,
                content: format!("Step {} executed: {}", step.id, result),
            });
        }

        // Phase 3: Synthesize final answer
        let conversation_history = self.memory.lock().unwrap().last_n(20)
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

        let answer = self.model.generate(&final_prompt, None).await
            .map_err(AgentError::Model)?;

        Ok(answer)
    }

    /// Execute a single tool by name.
    fn execute_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<String, AgentError> {
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
