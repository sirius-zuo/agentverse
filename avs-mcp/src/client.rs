use serde_json::json;
use serde_json::Value;
use uuid::Uuid;

/// MCP Client for connecting to MCP servers via SSE transport.
/// MVP supports only SSE (not stdio).
pub struct McpClient {
    server_url: String,
    client: reqwest::Client,
}

/// An MCP tool definition from the server
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, serde::Serialize)]
struct McpRequest {
    id: String,
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

impl McpClient {
    pub fn new(server_url: &str) -> Self {
        Self {
            server_url: server_url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Initialize connection with the MCP server.
    pub async fn initialize(&self) -> Result<(), McpError> {
        let request = McpRequest {
            id: Uuid::new_v4().to_string(),
            jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "agentverse",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        };

        let response = self.client
            .post(format!("{}/message", self.server_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

        if !response.status().is_success() {
            return Err(McpError::Initialization(response.status().as_u16()));
        }

        Ok(())
    }

    /// List available tools from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        let request = McpRequest {
            id: Uuid::new_v4().to_string(),
            jsonrpc: "2.0".to_string(),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = self.client
            .post(format!("{}/message", self.server_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

        let body: Value = response.json().await
            .map_err(|e| McpError::Parse(e.to_string()))?;

        let tools = body["result"]["tools"]
            .as_array()
            .ok_or_else(|| McpError::Parse("No tools array in response".to_string()))?;

        let mut result = Vec::new();
        for tool in tools {
            result.push(McpToolInfo {
                name: tool["name"].as_str().unwrap_or("").to_string(),
                description: tool["description"].as_str().unwrap_or("").to_string(),
                parameters: tool["inputSchema"].clone(),
            });
        }

        Ok(result)
    }

    /// Call an MCP tool.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, McpError> {
        let request = McpRequest {
            id: Uuid::new_v4().to_string(),
            jsonrpc: "2.0".to_string(),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": name,
                "arguments": args
            })),
        };

        let response = self.client
            .post(format!("{}/message", self.server_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

        let body: Value = response.json().await
            .map_err(|e| McpError::Parse(e.to_string()))?;

        if let Some(error) = body.get("error") {
            return Err(McpError::ToolCall(error["message"].as_str()
                .unwrap_or("Unknown error").to_string()));
        }

        Ok(body["result"]["content"][0]["text"].clone())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum McpError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Initialization failed with status: {0}")]
    Initialization(u16),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Tool call error: {0}")]
    ToolCall(String),
}
