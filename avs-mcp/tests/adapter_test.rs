use agentverse::ErasedTool;
use agentverse_mcp::adapter::McpToolAdapter;
use serde_json::json;
use std::sync::Arc;

#[test]
fn mcp_adapter_implements_erased_tool() {
    let client = Arc::new(agentverse_mcp::client::McpClient::new_disconnected_for_test());
    let adapter = McpToolAdapter::new(
        "test_tool".into(),
        "A test tool".into(),
        json!({ "type": "object", "properties": {} }),
        client,
    );
    let _erased: &dyn ErasedTool = &adapter;
    assert_eq!(adapter.name(), "test_tool");
    assert!(adapter.schema()["input_schema"].is_object());
}
