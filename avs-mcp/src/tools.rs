use super::client::McpClient;
use agentverse::{AsyncTool, ToolError, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Adapter that wraps an MCP tool as an AsyncTool.
pub struct McpToolAdapter {
    name: String,
    description: String,
    parameters: Value,
    client: Arc<McpClient>,
}

impl McpToolAdapter {
    pub fn new(
        name: String,
        description: String,
        parameters: Value,
        client: Arc<McpClient>,
    ) -> Self {
        Self {
            name,
            description,
            parameters,
            client,
        }
    }
}

#[async_trait]
impl AsyncTool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    async fn execute(&self, args: Value) -> ToolResult {
        self.client
            .call_tool(&self.name, args)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))
    }
}
