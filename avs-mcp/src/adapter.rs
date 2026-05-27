use agentverse::{ErasedTool, ToolError, ToolResult};
use serde_json::Value;
use std::sync::Arc;

use crate::client::McpClient;

/// Wraps a remote MCP tool as a native ErasedTool.
/// Schema comes from the MCP server; args are forwarded as raw JSON.
pub struct McpToolAdapter {
    tool_name: String,
    tool_description: String,
    input_schema: Value,
    client: Arc<McpClient>,
}

impl McpToolAdapter {
    pub fn new(
        name: String,
        description: String,
        input_schema: Value,
        client: Arc<McpClient>,
    ) -> Self {
        Self {
            tool_name: name,
            tool_description: description,
            input_schema,
            client,
        }
    }
}

#[async_trait::async_trait]
impl ErasedTool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": self.tool_name,
            "description": self.tool_description,
            "input_schema": self.input_schema,
        })
    }

    async fn execute_raw(&self, args: Value) -> ToolResult {
        self.client
            .call_tool(&self.tool_name, args)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))
    }
}
