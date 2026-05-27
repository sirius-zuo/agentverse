use std::sync::Arc;
use crate::registry::ToolRegistry;
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
pub struct FindToolsArgs {
    /// Natural language description of the capability you need
    pub query: String,
    /// Maximum number of results (default 5)
    #[serde(default = "default_limit")]
    pub limit: u8,
}
fn default_limit() -> u8 {
    5
}

pub struct FindToolsTool {
    registry: Arc<ToolRegistry>,
}

impl FindToolsTool {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl Tool for FindToolsTool {
    type Args = FindToolsArgs;
    fn name(&self) -> &str {
        "find_tools"
    }
    fn description(&self) -> &str {
        "Search the tool registry by keyword to discover available tools"
    }
    async fn execute(&self, args: FindToolsArgs) -> ToolResult {
        // Stub — full implementation in Task 4
        Ok(json!({ "tools": [], "query": args.query }))
    }
}
