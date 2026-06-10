use crate::context::{ResourceContent, SubAgentContext};
use crate::executor::SubAgentExecutor;
use crate::spec::{Budget, ModelOverride, SubAgentSpec};
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

pub struct SubAgentTool {
    executor: Arc<SubAgentExecutor>,
    current_depth: usize,
}

impl SubAgentTool {
    pub fn new(executor: Arc<SubAgentExecutor>, current_depth: usize) -> Self {
        Self {
            executor,
            current_depth,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubAgentArgs {
    /// Short identifier for this SubAgent (used in tracing).
    pub name: String,
    /// What the SubAgent should accomplish.
    pub objective: String,
    /// Optional system prompt override.
    pub system_prompt: Option<String>,
    /// Model alias: "haiku", "sonnet", or "opus". Omit to inherit parent model.
    pub model: Option<String>,
    /// Tool names this SubAgent may use. Empty list = reasoning only (no tools).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Maximum ReAct steps. Defaults to 10.
    pub max_steps: Option<usize>,
    /// Maximum total tokens (input + output). Defaults to 20000.
    pub max_tokens: Option<u32>,
    /// Timeout in seconds. Defaults to 120.
    pub timeout_secs: Option<u64>,
    /// Context to inject: files, prior results, etc.
    pub resources: Vec<ResourceArgJson>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResourceArgJson {
    pub label: String,
    pub content: String,
}

#[async_trait::async_trait]
impl Tool for SubAgentTool {
    type Args = SubAgentArgs;

    fn name(&self) -> &str {
        "spawn_subagent"
    }

    fn description(&self) -> &str {
        "Spawn an isolated AI worker to handle a focused subtask. \
         The worker runs independently and returns only its final answer. \
         Use for tasks that would add too much noise to the current context, \
         or when you need to run multiple analyses in parallel."
    }

    async fn execute(&self, args: SubAgentArgs) -> ToolResult {
        let agent_name = args.name.clone();
        let spec = SubAgentSpec {
            name: args.name,
            objective: args.objective,
            system_prompt: args.system_prompt,
            model: args.model.map(ModelOverride::Alias),
            allowed_tools: args.allowed_tools,
            budget: Budget {
                max_steps: args.max_steps.unwrap_or(10),
                max_tokens: args.max_tokens.unwrap_or(20_000),
                timeout: Duration::from_secs(args.timeout_secs.unwrap_or(120)),
            },
        };
        let ctx = SubAgentContext {
            resources: args
                .resources
                .into_iter()
                .map(|r| ResourceContent {
                    label: r.label,
                    content: r.content,
                })
                .collect(),
            depth: self.current_depth,
        };
        match self.executor.run(&spec, ctx).await {
            Ok(result) => {
                tracing::info!(
                    name = %agent_name,
                    steps = result.steps,
                    tokens = result.usage.input_tokens + result.usage.output_tokens,
                    "subagent finished"
                );
                Ok(json!(result.answer))
            }
            Err(e) => {
                tracing::warn!(name = %agent_name, error = %e, "subagent failed");
                Err(agentverse::ToolError::Execution(e.to_string()))
            }
        }
    }
}
