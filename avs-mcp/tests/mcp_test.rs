use agentverse::AsyncTool;
use agentverse_mcp::{McpClient, McpError, McpToolAdapter, McpToolInfo};
use serde_json::json;

#[test]
fn test_mcp_client_creation() {
    let _client = McpClient::new("http://localhost:3000/message");
    // Client created successfully — construction validates server_url parsing
}

// Note: Integration tests for initialize, list_tools, and call_tool
// require a running MCP server. Those are skipped in unit tests.

#[test]
fn test_mcp_tool_info() {
    use serde_json::json;
    let info = McpToolInfo {
        name: "test_tool".to_string(),
        description: "A test tool".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {}
        }),
    };
    assert_eq!(info.name, "test_tool");
    assert_eq!(info.description, "A test tool");
}

#[test]
fn test_mcp_error_display() {
    let err = McpError::Connection("connection refused".to_string());
    assert!(err.to_string().contains("connection refused"));

    let err = McpError::Initialization(404);
    assert!(err.to_string().contains("404"));

    let err = McpError::Parse("invalid json".to_string());
    assert!(err.to_string().contains("invalid json"));

    let err = McpError::ToolCall("tool not found".to_string());
    assert!(err.to_string().contains("tool not found"));
}

#[test]
fn test_mcp_tool_adapter_trait_methods() {
    let client = McpClient::new("http://localhost:3000/message");
    let adapter = McpToolAdapter::new(
        "web_search".to_string(),
        "Search the web for information".to_string(),
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        }),
        std::sync::Arc::new(client),
    );

    assert_eq!(adapter.name(), "web_search");
    assert_eq!(adapter.description(), "Search the web for information");
    assert_eq!(
        adapter.parameters()["required"].as_array().unwrap().len(),
        1
    );
}

#[tokio::test]
async fn test_mcp_tool_adapter_is_async_tool() {
    // Verifies McpToolAdapter implements AsyncTool (trait methods accessible without a runtime hack)
    let client = McpClient::new("http://localhost:3000/message");
    let adapter = McpToolAdapter::new(
        "async_tool".to_string(),
        "An async tool".to_string(),
        json!({"type": "object", "properties": {}}),
        std::sync::Arc::new(client),
    );

    // Trait methods must be reachable via AsyncTool
    let tool: &dyn AsyncTool = &adapter;
    assert_eq!(tool.name(), "async_tool");
    assert_eq!(tool.description(), "An async tool");
    assert_eq!(tool.parameters()["type"], "object");
}
