use agentverse::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
pub struct CheckpointArgs {
    /// Named checkpoint declared in skill frontmatter.
    pub name: String,
    /// Payload for the human reviewer.
    pub payload: Value,
}

pub struct RequestCheckpointTool;

#[async_trait::async_trait]
impl Tool for RequestCheckpointTool {
    type Args = CheckpointArgs;
    fn name(&self) -> &str {
        "request_checkpoint"
    }
    fn description(&self) -> &str {
        "Pause execution and request human approval at a named checkpoint."
    }
    async fn execute(&self, _args: CheckpointArgs) -> agentverse::ToolResult {
        // Execution is intercepted by HitlContext before this is called.
        // This body is a safety fallback and should never run in production.
        Ok(serde_json::json!({ "status": "checkpoint_intercepted" }))
    }
}
