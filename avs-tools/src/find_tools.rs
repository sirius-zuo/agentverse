use crate::registry::ToolRegistry;
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

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
        "Search the tool registry by keyword to discover available tools and their capabilities"
    }

    async fn execute(&self, args: FindToolsArgs) -> ToolResult {
        let limit = args.limit as usize;
        let results = self.registry.search(&args.query, limit);
        let tools: Vec<serde_json::Value> = results
            .into_iter()
            .map(|info| {
                json!({
                    "name": info.name,
                    "description": info.description,
                    "score": info.score,
                })
            })
            .collect();
        Ok(json!({ "query": args.query, "tools": tools }))
    }
}
