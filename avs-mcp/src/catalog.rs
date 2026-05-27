use agentverse_tools::{ToolOptions, ToolRegistry};
use std::sync::Arc;

use crate::adapter::McpToolAdapter;
use crate::client::McpClient;
use crate::error::McpError;

pub struct McpCatalogSource;

impl McpCatalogSource {
    /// List all tools from an MCP server and register them into the registry.
    /// Returns the number of tools registered.
    pub async fn populate(
        registry: &Arc<ToolRegistry>,
        client: &Arc<McpClient>,
    ) -> Result<usize, McpError> {
        let tools = client.list_tools().await?;
        let count = tools.len();
        for info in tools {
            let adapter = McpToolAdapter::new(
                info.name,
                info.description,
                info.input_schema,
                Arc::clone(client),
            );
            registry.register_erased(
                Arc::new(adapter),
                ToolOptions {
                    category: Some("mcp".into()),
                    ..Default::default()
                },
            );
        }
        Ok(count)
    }
}
