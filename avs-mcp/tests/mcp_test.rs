use agentverse_mcp::{McpClient, McpError, McpToolInfo};

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
