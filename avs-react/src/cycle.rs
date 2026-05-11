//! Shared cycle skeleton used by all orchestration strategies.
//!
//! Provides the fixed loop structure; each strategy only implements
//! its own `step()` logic via a closure.

use agentverse::{AgentError, Message, ModelProvider, PromptRegistry, SyncTool};
use agentverse_guardrails::{check_output, check_prompt};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info};

/// The fixed cycle skeleton that all strategies share.
///
/// Each strategy provides its own `step()` closure that decides
/// what happens on each iteration.
pub struct CycleSkeleton<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    prompt_registry: Arc<PromptRegistry>,
    model: Arc<P>,
    tools: Vec<Box<dyn SyncTool>>,
    memory: Arc<Mutex<M>>,
    max_iterations: usize,
    current_iteration: usize,
}

/// Represents the strategy's decision for the next action.
#[derive(Debug)]
pub enum CycleAction {
    /// LLM said "think" — continue the loop with a thought.
    Continue { thought: String },
    /// LLM decided to call a tool.
    ToolCall { tool_name: String, args: Value },
    /// LLM provided a final answer.
    Done { answer: String },
    /// LLM indicated an error.
    Error { message: String },
}

impl<P, M> CycleSkeleton<P, M>
where
    P: ModelProvider,
    M: agentverse::Memory,
{
    /// Create a new cycle skeleton.
    pub fn new(
        prompt_registry: Arc<PromptRegistry>,
        model: Arc<P>,
        tools: Vec<Box<dyn SyncTool>>,
        memory: Arc<Mutex<M>>,
        max_iterations: usize,
    ) -> Self {
        Self {
            prompt_registry,
            model,
            tools,
            memory,
            max_iterations,
            current_iteration: 0,
        }
    }

    /// Run the strategy loop (async).
    ///
    /// Each strategy provides its own `step` closure that returns a `CycleAction`.
    pub async fn run<F, Fut>(
        &mut self,
        initial_message: String,
        mut step: F,
    ) -> Result<String, AgentError>
    where
        F: FnMut(&mut Self) -> Fut,
        Fut: std::future::Future<Output = Result<CycleAction, AgentError>>,
    {
        self.memory.lock().unwrap().append(Message {
            role: agentverse::memory::MessageRole::User,
            content: initial_message,
        });

        while self.current_iteration < self.max_iterations {
            self.current_iteration += 1;
            debug!(iteration = self.current_iteration, "Running strategy step");

            let action = step(self).await?;

            match action {
                CycleAction::Continue { thought } => {
                    self.memory.lock().unwrap().append(Message {
                        role: agentverse::memory::MessageRole::Assistant,
                        content: format!("Thought: {}", thought),
                    });
                    info!(
                        iteration = self.current_iteration,
                        "Thought only, continuing"
                    );
                }
                CycleAction::ToolCall { tool_name, args } => {
                    let result = self.execute_tool(&tool_name, args)?;
                    self.memory.lock().unwrap().append(Message {
                        role: agentverse::memory::MessageRole::Tool,
                        content: format!("Tool: {}\nResult: {}", tool_name, result),
                    });
                    info!(
                        iteration = self.current_iteration,
                        tool = tool_name,
                        "Tool executed"
                    );
                }
                CycleAction::Done { answer } => {
                    self.memory.lock().unwrap().append(Message {
                        role: agentverse::memory::MessageRole::Assistant,
                        content: answer.clone(),
                    });
                    info!(iteration = self.current_iteration, "Strategy completed");
                    return Ok(answer);
                }
                CycleAction::Error { message } => {
                    error!(error = %message, "Strategy error");
                    return Err(AgentError::Model(agentverse::ModelError::InvalidResponse(
                        message,
                    )));
                }
            }
        }

        Err(AgentError::Model(agentverse::ModelError::Timeout(format!(
            "Max iterations ({}) reached",
            self.max_iterations
        ))))
    }

    /// Execute a tool by name with the given arguments.
    pub fn execute_tool(&self, tool_name: &str, args: Value) -> Result<String, AgentError> {
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

    /// Build the prompt for the LLM from conversation history and tool descriptions.
    pub fn build_prompt(&self) -> Result<String, AgentError> {
        let last_messages = self.memory.lock().unwrap().last_n(20);
        let mut context = HashMap::new();

        // Format conversation as a string for the template
        let conversation: String = last_messages
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
        context.insert("conversation".to_string(), Value::String(conversation));

        // Format tools as a string
        let tools: String = self
            .tools
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");
        context.insert("tools".to_string(), Value::String(tools));

        self.prompt_registry.render("react", context)
    }

    /// Build the prompt with guardrail checking on the rendered prompt.
    pub fn build_prompt_with_guardrails(&self) -> Result<String, AgentError> {
        let prompt = self.build_prompt()?;
        check_prompt(&prompt).map_err(|e| match e {
            agentverse_guardrails::GuardrailError::PromptInjection(msg) => {
                AgentError::Guardrail(agentverse::GuardrailError::PromptInjection(msg))
            }
            agentverse_guardrails::GuardrailError::OutputFiltered(msg) => {
                AgentError::Guardrail(agentverse::GuardrailError::OutputFiltered(msg))
            }
            _ => AgentError::Guardrail(agentverse::GuardrailError::PromptInjection(e.to_string())),
        })?;
        Ok(prompt)
    }

    /// Apply output guardrail to a model response.
    pub fn check_output_guardrail(&self, output: &str) -> Result<(), AgentError> {
        check_output(output).map_err(|e| match e {
            agentverse_guardrails::GuardrailError::OutputFiltered(msg) => {
                AgentError::Guardrail(agentverse::GuardrailError::OutputFiltered(msg))
            }
            agentverse_guardrails::GuardrailError::PromptInjection(msg) => {
                AgentError::Guardrail(agentverse::GuardrailError::PromptInjection(msg))
            }
            _ => AgentError::Guardrail(agentverse::GuardrailError::OutputFiltered(e.to_string())),
        })
    }

    /// Return the number of tools available.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Return the current iteration count.
    pub fn current_iteration(&self) -> usize {
        self.current_iteration
    }

    /// Return max iterations.
    pub fn max_iterations(&self) -> usize {
        self.max_iterations
    }

    /// Return a reference to the model.
    pub fn model(&self) -> &P {
        &self.model
    }

    /// Return a reference to the tools.
    pub fn tools(&self) -> &[Box<dyn SyncTool>] {
        &self.tools
    }

    /// Return a reference to the memory.
    pub fn memory(&self) -> &Arc<Mutex<M>> {
        &self.memory
    }

    /// Return a reference to the prompt registry.
    pub fn prompt_registry(&self) -> &Arc<PromptRegistry> {
        &self.prompt_registry
    }

    /// Increment the iteration counter and return the new value.
    pub fn next_iteration(&mut self) -> usize {
        self.current_iteration += 1;
        self.current_iteration
    }
}
