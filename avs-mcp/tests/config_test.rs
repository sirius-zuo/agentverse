use agentverse_mcp::config::{McpServerConfig, TransportKind};

#[test]
fn deserialize_stdio_config() {
    let toml = r#"
        name = "github"
        transport = "stdio"
        command = "npx"
        args = ["-y", "@modelcontextprotocol/server-github"]
        env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }
    "#;
    // We expect this to parse even with unexpanded var
    let config: McpServerConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.name, "github");
    assert!(matches!(config.transport, TransportKind::Stdio));
    assert_eq!(config.command.unwrap(), "npx");
}

#[test]
fn deserialize_http_config() {
    let toml = r#"
        name = "remote"
        transport = "streamable_http"
        url = "https://tools.example.com/mcp"
        headers = { Authorization = "Bearer token123" }
    "#;
    let config: McpServerConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.name, "remote");
    assert!(matches!(config.transport, TransportKind::StreamableHttp));
}

#[test]
fn env_var_expansion_replaces_vars() {
    std::env::set_var("TEST_MCP_SECRET", "abc123");
    let toml = r#"
        name = "test"
        transport = "stdio"
        command = "server"
        env = { SECRET = "${TEST_MCP_SECRET}" }
    "#;
    let config: McpServerConfig = toml::from_str(toml).unwrap();
    let transport = config.into_transport().unwrap();
    if let agentverse_mcp::McpTransport::Stdio { env, .. } = transport {
        assert_eq!(env["SECRET"], "abc123");
    } else {
        panic!("expected stdio");
    }
}

#[test]
fn missing_env_var_returns_error() {
    let toml = r#"
        name = "test"
        transport = "stdio"
        command = "server"
        env = { SECRET = "${DEFINITELY_UNDEFINED_VAR_XYZ}" }
    "#;
    let config: McpServerConfig = toml::from_str(toml).unwrap();
    let result = config.into_transport();
    assert!(result.is_err());
}
