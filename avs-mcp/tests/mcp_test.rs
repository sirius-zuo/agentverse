use agentverse::SyncTool;
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

#[test]
fn test_mcp_tool_adapter_no_runtime_error() {
    // Execute outside a tokio runtime to verify the error path
    let client = McpClient::new("http://localhost:3000/message");
    let adapter = McpToolAdapter::new(
        "test_tool".to_string(),
        "A test tool".to_string(),
        json!({"type": "object"}),
        std::sync::Arc::new(client),
    );

    let result = adapter.execute(json!({}));
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("No tokio runtime available"));
}
