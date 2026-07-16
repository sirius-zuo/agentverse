use agentverse::{Tool, ToolResult};
use agentverse_mcp::{McpError, McpLoader, McpServer, McpServerConfig};
use agentverse_tools::ToolRegistry;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
struct McpConfig {
    mcp_servers: Vec<McpServerConfig>,
}

#[derive(Deserialize, JsonSchema)]
struct EchoArgs {
    message: String,
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
        Ok(json!({ "echo": args.message }))
    }
}

#[tokio::test]
async fn loader_registers_mcp_adapter_from_deserialized_config() {
    let server_registry = ToolRegistry::new();
    server_registry.register(EchoTool);

    let mut server = McpServer::new(Arc::clone(&server_registry));
    let port = server.bind_random_port().await.unwrap();
    tokio::spawn(async move { server.run().await });

    let config: McpConfig = toml::from_str(&format!(
        r#"
            [[mcp_servers]]
            name = "local-echo"
            transport = "streamable_http"
            url = "http://127.0.0.1:{port}/mcp"
        "#
    ))
    .unwrap();
    let client_registry = ToolRegistry::new();

    let loaded = McpLoader::load(&client_registry, &config.mcp_servers)
        .await
        .unwrap();

    assert_eq!(loaded, 1);
    assert!(client_registry
        .tool_names()
        .iter()
        .any(|name| name == "echo"));
    assert!(client_registry
        .filter_category("mcp")
        .tool_names()
        .iter()
        .any(|name| name == "echo"));
    assert_eq!(
        client_registry
            .execute("echo", json!({ "message": "hello" }))
            .await
            .unwrap(),
        json!({ "echo": "hello" }).to_string()
    );
}

#[tokio::test]
async fn loader_returns_config_error_for_malformed_server_config() {
    let config: McpConfig = toml::from_str(
        r#"
            [[mcp_servers]]
            name = "missing-url"
            transport = "streamable_http"
        "#,
    )
    .unwrap();

    let registry = ToolRegistry::new();
    let result = McpLoader::load(&registry, &config.mcp_servers).await;

    assert!(matches!(result, Err(McpError::Config(message)) if message.contains("missing-url")));
}
