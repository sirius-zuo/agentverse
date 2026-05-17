//! Shared cycle skeleton used by all orchestration strategies.
//!
//! Provides the fixed loop structure; each strategy only implements
//! its own `step()` logic via a closure.

use agentverse::{AgentError, Message, ModelProvider, PromptRegistry, SyncTool};
use agentverse_guardrails::{check_output, check_prompt};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
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
    total_usage: agentverse::UsageStats,
    /// Set after the react preamble has been pushed to memory, so it only
    /// happens once per agent lifetime and build_request() knows to skip
    /// embedding tools in the system prompt.
    react_primed: bool,
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
            total_usage: agentverse::UsageStats::default(),
            react_primed: false,
        }
    }

    /// Run the strategy loop (async).
    ///
    /// Each strategy provides its own `step` closure that returns a `CycleAction`.
    pub async fn run<F, Fut>(
        &mut self,
        initial_message: String,
        mut step: F,
    ) -> Result<agentverse::CycleResult, AgentError>
    where
        F: FnMut(&mut Self) -> Fut,
        Fut: std::future::Future<Output = Result<CycleAction, AgentError>>,
    {
        self.memory.lock().await.append(Message {
            role: agentverse::memory::MessageRole::User,
            content: initial_message,
        });

        while self.current_iteration < self.max_iterations {
            self.current_iteration += 1;
            debug!(iteration = self.current_iteration, "Running strategy step");

            let action = step(self).await?;

            match action {
                CycleAction::Continue { thought } => {
                    self.memory.lock().await.append(Message {
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
                    self.memory.lock().await.append(Message {
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
                    self.memory.lock().await.append(Message {
                        role: agentverse::memory::MessageRole::Assistant,
                        content: answer.clone(),
                    });
                    info!(iteration = self.current_iteration, "Strategy completed");
                    return Ok(agentverse::CycleResult {
                        answer,
                        total_usage: self.total_usage,
                    });
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

    /// Render tool descriptions as a human-readable string for prompt injection.
    fn build_tools_str(&self) -> String {
        self.tools
            .iter()
            .map(|t| {
                let params = t.parameters();
                let props = params["properties"].as_object();
                let required: Vec<&str> = params["required"]
                    .as_array()
                    .map(|r| r.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                if let Some(props) = props {
                    let param_lines = props
                        .iter()
                        .map(|(k, v)| {
                            let req = if required.contains(&k.as_str()) {
                                "required"
                            } else {
                                "optional"
                            };
                            let desc = v["description"].as_str().unwrap_or("");
                            format!("    - {} ({}): {}", k, req, desc)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "- {}: {}\n  Parameters:\n{}",
                        t.name(),
                        t.description(),
                        param_lines
                    )
                } else {
                    format!("- {}: {}", t.name(), t.description())
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Prime the conversation with the react preamble if a `react.j2` file was
    /// loaded into the registry.  Called once before the first user message.
    ///
    /// The rendered preamble contains tool descriptions and few-shot examples.
    /// Because it is inserted as the first `User` message and never changes, it
    /// stays inside the stable prefix that the penultimate-message cache
    /// breakpoint captures on every subsequent turn — it is effectively free
    /// after the first round-trip.
    pub async fn prime_react_preamble(&mut self) {
        if self.react_primed || !self.prompt_registry.has_react_template() {
            return;
        }

        let tools_str = self.build_tools_str();

        let mut ctx = std::collections::HashMap::new();
        ctx.insert("tools".to_string(), serde_json::Value::String(tools_str));

        // Inject "react_examples" set if present, under the key "examples".
        if let Some(examples) = self.prompt_registry.get_examples("react_examples") {
            if let Ok(val) = serde_json::to_value(examples) {
                ctx.insert("examples".to_string(), val);
            }
        }

        if let Ok(preamble) = self.prompt_registry.render("react", ctx) {
            if !preamble.trim().is_empty() {
                self.memory.lock().await.pin(vec![agentverse::Message {
                    role: agentverse::memory::MessageRole::User,
                    content: preamble,
                }]);
            }
        }

        self.react_primed = true;
    }

    /// Returns true after `prime_react_preamble` has run successfully.
    pub fn is_react_primed(&self) -> bool {
        self.react_primed
    }

    /// Build a structured GenerateRequest from the system template, memory, and tools.
    ///
    /// When the react preamble is active (`react_primed`), tool descriptions
    /// live in the stable preamble message and are NOT duplicated in the system
    /// prompt.  When there is no react preamble, tools are embedded in the
    /// system prompt as before (backward-compatible with examples that use the
    /// default registry).
    pub async fn build_request(&self) -> Result<agentverse::GenerateRequest, AgentError> {
        let mut sys_context = std::collections::HashMap::new();
        if !self.react_primed {
            // No react preamble — embed tools in system prompt (default behaviour).
            sys_context.insert(
                "tools".to_string(),
                serde_json::Value::String(self.build_tools_str()),
            );
        }
        let system = self.prompt_registry.render("system", sys_context).ok();

        let messages = self
            .memory
            .lock()
            .await
            .last_n(20)
            .await
            .map_err(|e| AgentError::Memory(e.to_string()))?;

        // ReAct uses text-based tool descriptions — do not send native tool
        // schemas, which would cause the model to emit tool_calls instead of
        // Thought/Action/Answer text.
        Ok(agentverse::GenerateRequest {
            system,
            messages,
            tools: None,
        })
    }

    /// Build the request with guardrail checking on the rendered system prompt.
    pub async fn build_request_with_guardrails(&self) -> Result<agentverse::GenerateRequest, AgentError> {
        let request = self.build_request().await?;
        if let Some(ref system) = request.system {
            check_prompt(system).map_err(|e| match e {
                agentverse_guardrails::GuardrailError::PromptInjection(msg) => {
                    AgentError::Guardrail(agentverse::GuardrailError::PromptInjection(msg))
                }
                agentverse_guardrails::GuardrailError::OutputFiltered(msg) => {
                    AgentError::Guardrail(agentverse::GuardrailError::OutputFiltered(msg))
                }
                _ => AgentError::Guardrail(agentverse::GuardrailError::PromptInjection(
                    e.to_string(),
                )),
            })?;
        }
        Ok(request)
    }

    /// Accumulate usage stats from a single generate() call.
    pub fn accumulate_usage(&mut self, usage: agentverse::UsageStats) {
        self.total_usage += usage;
    }

    /// Return accumulated usage stats across all iterations.
    pub fn total_usage(&self) -> agentverse::UsageStats {
        self.total_usage
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
