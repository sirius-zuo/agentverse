use agentverse::{Tool, ToolResult};
use agentverse_mcp::{McpClient, McpServer, McpTransport};
use agentverse_tools::ToolRegistry;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize, JsonSchema)]
struct EchoArgs {
    msg: String,
}

struct EchoTool;

#[async_trait::async_trait]
impl Tool for EchoTool {
    type Args = EchoArgs;
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "Echo a message"
    }
    async fn execute(&self, args: EchoArgs) -> ToolResult {
        Ok(json!({ "echo": args.msg }))
    }
}

#[tokio::test]
async fn server_lists_registered_tools() {
    let registry = ToolRegistry::new();
    registry.register(EchoTool);

    let mut server = McpServer::new(Arc::clone(&registry));
    let port = server.bind_random_port().await.unwrap();

    tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let transport = McpTransport::StreamableHttp {
        endpoint: format!("http://127.0.0.1:{port}/mcp").parse().unwrap(),
        headers: Default::default(),
    };
    let client = McpClient::connect(transport).await.unwrap();
    let tools = client.list_tools().await.unwrap();
    assert!(tools.iter().any(|t| t.name == "echo"));
}

#[tokio::test]
async fn server_executes_tool() {
    let registry = ToolRegistry::new();
    registry.register(EchoTool);

    let mut server = McpServer::new(Arc::clone(&registry));
    let port = server.bind_random_port().await.unwrap();
    tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let transport = McpTransport::StreamableHttp {
        endpoint: format!("http://127.0.0.1:{port}/mcp").parse().unwrap(),
        headers: Default::default(),
    };
    let client = McpClient::connect(transport).await.unwrap();
    let result = client.call_tool("echo", json!({"msg": "hello"})).await.unwrap();
    assert_eq!(result, json!({ "echo": "hello" }).to_string());
}
