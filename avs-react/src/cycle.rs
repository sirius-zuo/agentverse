//! Shared cycle skeleton used by all orchestration strategies.
//!
//! Provides the fixed loop structure; each strategy only implements
//! its own `step()` logic via a closure.

use agentverse::{AgentError, LlmRunner, PromptRegistry, ToolCall, ToolCallResult};
use agentverse_guardrails::check_output;
use agentverse_tools::{ActiveToolSet, ToolRegistry};
use serde_json::Value;
use std::sync::Arc;

/// The fixed cycle skeleton that all strategies share.
///
/// Each strategy provides its own `step()` closure that decides
/// what happens on each iteration.
pub struct CycleSkeleton {
    pub runner: Arc<LlmRunner>,
    pub prompts: Arc<PromptRegistry>,
    pub tools: Arc<ToolRegistry>,
    max_iterations: usize,
}

/// Represents the strategy's decision for the next action.
#[derive(Debug)]
pub enum CycleAction {
    /// LLM said "think" — continue the loop with a thought.
    Continue { thought: String },
    /// LLM decided to call a single tool.
    ToolCall { tool_name: String, args: Value },
    /// LLM decided to call multiple tools in parallel.
    ToolCalls { calls: Vec<ToolCall> },
    /// LLM provided a final answer.
    Done { answer: String },
    /// LLM indicated an error.
    Error { message: String },
}

impl CycleSkeleton {
    /// Create a new cycle skeleton.
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

    /// Execute a tool by name with the given arguments.
    pub async fn execute_tool(&self, tool_name: &str, args: Value) -> Result<String, AgentError> {
        let result = self
            .tools
            .execute(tool_name, args)
            .await
            .map_err(AgentError::Tool)?;
        Ok(result.to_string())
    }

    /// Render tool descriptions as a human-readable string for prompt injection.
    pub fn build_tools_str(&self) -> String {
        self.tools
            .schema()
            .into_iter()
            .map(|schema| {
                let name = schema["name"].as_str().unwrap_or("");
                let description = schema["description"].as_str().unwrap_or("");
                let input = &schema["input_schema"];
                let props = input["properties"].as_object();
                let required: Vec<&str> = input["required"]
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
                        name, description, param_lines
                    )
                } else {
                    format!("- {}: {}", name, description)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Optionally insert the ReAct preamble into a message buffer.
    ///
    /// If a `react.j2` template is registered, the rendered preamble (containing
    /// tool descriptions and few-shot examples) is inserted before the first
    /// non-system message.  When no template is present the buffer is returned
    /// unchanged — this keeps the method safe for non-ReAct strategies.
    pub fn prepare_buffer(&self, messages: Vec<agentverse::Message>) -> Vec<agentverse::Message> {
        if !self.prompts.has_react_template() {
            return messages;
        }

        let tools_str = self.build_tools_str();
        let mut ctx = std::collections::HashMap::new();
        ctx.insert("tools".to_string(), serde_json::Value::String(tools_str));

        // Inject "react_examples" set if present, under the key "examples".
        if let Some(examples) = self.prompts.get_examples("react_examples") {
            if let Ok(val) = serde_json::to_value(examples) {
                ctx.insert("examples".to_string(), val);
            }
        }

        let mut buf = messages;
        if let Ok(preamble) = self.prompts.render("react", ctx) {
            if !preamble.trim().is_empty() {
                let insert_pos = buf
                    .iter()
                    .position(|m| !matches!(m.role, agentverse::MessageRole::System))
                    .unwrap_or(0);
                buf.insert(
                    insert_pos,
                    agentverse::Message {
                        role: agentverse::MessageRole::User,
                        content: preamble,
                    },
                );
            }
        }

        buf
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

    /// Return the number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Execute multiple tool calls concurrently.
    pub async fn execute_many(
        &self,
        calls: Vec<ToolCall>,
    ) -> Result<Vec<ToolCallResult>, AgentError> {
        Ok(self.tools.execute_many(calls).await)
    }

    /// Build tools string scoped to an ActiveToolSet.
    pub fn build_tools_str_active(&self, active: &ActiveToolSet) -> String {
        active
            .schemas(&self.tools)
            .into_iter()
            .map(|schema| {
                let name = schema["name"].as_str().unwrap_or("");
                let description = schema["description"].as_str().unwrap_or("");
                let input = &schema["input_schema"];
                let props = input["properties"].as_object();
                let required: Vec<&str> = input["required"]
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
                        name, description, param_lines
                    )
                } else {
                    format!("- {}: {}", name, description)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Return max iterations.
    pub fn max_iterations(&self) -> usize {
        self.max_iterations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

    fn make_skeleton() -> CycleSkeleton {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::openai(
                    "test".to_string(),
                    "sk-test".to_string(),
                    Some("http://127.0.0.1:1/v1".to_string()),
                ),
                max_messages: 10,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap(),
        );
        CycleSkeleton::new(
            runner,
            Arc::new(PromptRegistry::new()),
            ToolRegistry::new(),
            5,
        )
    }

    #[test]
    fn skeleton_tool_count_has_find_tools() {
        let s = make_skeleton();
        // find_tools is auto-registered, so count is at least 1
        assert!(s.tool_count() >= 1);
    }

    #[test]
    fn skeleton_max_iterations() {
        let s = make_skeleton();
        assert_eq!(s.max_iterations(), 5);
    }

    #[test]
    fn skeleton_prepare_buffer_no_preamble() {
        let s = make_skeleton();
        let msgs = vec![agentverse::Message {
            role: agentverse::MessageRole::User,
            content: "hi".to_string(),
        }];
        let buf = s.prepare_buffer(msgs);
        // Without a react prompt template, buffer is unchanged
        assert_eq!(buf.len(), 1);
    }
}
