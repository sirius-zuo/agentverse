use super::client::McpClient;
use agentverse::{SyncTool, ToolResult};
use serde_json::Value;
use std::sync::Arc;

/// Adapter that wraps an MCP tool as a SyncTool.
/// Executes MCP tools via the client.
pub struct McpToolAdapter {
    name: String,
    description: String,
    parameters: Value,
    client: Arc<McpClient>,
}

impl McpToolAdapter {
    pub fn new(name: String, description: String, parameters: Value, client: Arc<McpClient>) -> Self {
        Self {
            name,
            description,
            parameters,
            client,
        }
    }
}

impl SyncTool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    fn execute(&self, args: Value) -> ToolResult {
        // MCP execution is async, so we spawn a blocking task
        // In production, use a runtime handle
        let client = Arc::clone(&self.client);
        let name = self.name.clone();
        let args = args.clone();

        // Use tokio::runtime::Handle to run async in sync context
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let rt = handle;
                // Block on the async call — this is a limitation of the
                // SyncTool trait. In production, consider using AsyncTool
                // for MCP tools.
                rt.block_on(async {
                    client.call_tool(&name, args).await
                        .map_err(|e| agentverse::ToolError::Execution(e.to_string()))
                })
            }
            Err(_) => Err(agentverse::ToolError::Execution(
                "No tokio runtime available for MCP tool execution".to_string()
            )),
        }
    }
}
