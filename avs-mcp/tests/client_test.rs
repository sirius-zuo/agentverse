use agentverse_mcp::{McpClient, McpError, McpTransport};

#[tokio::test]
async fn streamable_http_connection_error_is_wrapped() {
    let transport = McpTransport::StreamableHttp {
        endpoint: "http://127.0.0.1:19999/mcp".parse().unwrap(),
        headers: Default::default(),
    };
    let result = McpClient::connect(transport).await;
    assert!(matches!(result, Err(McpError::Connection(_))));
}

#[test]
fn mcp_transport_variants_exist() {
    let _stdio = McpTransport::Stdio {
        command: std::path::PathBuf::from("./server"),
        args: vec!["--flag".into()],
        env: Default::default(),
    };
    let _http = McpTransport::StreamableHttp {
        endpoint: "https://example.com/mcp".parse().unwrap(),
        headers: Default::default(),
    };
}
