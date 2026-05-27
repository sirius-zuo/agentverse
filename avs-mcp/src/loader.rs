use agentverse_tools::ToolRegistry;
use std::sync::Arc;

use crate::catalog::McpCatalogSource;
use crate::client::McpClient;
use crate::config::McpServerConfig;
use crate::error::McpError;

pub struct McpLoader;

impl McpLoader {
    /// Connect to each configured MCP server and populate the registry.
    /// Returns the total number of tools registered across all servers.
    pub async fn load(
        registry: &Arc<ToolRegistry>,
        servers: &[McpServerConfig],
    ) -> Result<usize, McpError> {
        let mut total = 0usize;
        for config in servers {
            let name = config.name.clone();
            tracing::info!(server = %name, "Connecting to MCP server");
            let transport = config.into_transport().map_err(|e| {
                tracing::error!(server = %name, error = %e, "Config error");
                e
            })?;
            let client = McpClient::connect(transport).await.map_err(|e| {
                tracing::error!(server = %name, error = %e, "Connection failed");
                e
            })?;
            let count = McpCatalogSource::populate(registry, &client).await?;
            tracing::info!(server = %name, tools = count, "MCP server loaded");
            total += count;
        }
        Ok(total)
    }
}
