use std::collections::HashMap;
use std::path::PathBuf;
use reqwest::header::HeaderMap;

#[derive(Debug, Clone)]
pub enum McpTransport {
    /// Spawn a local subprocess and communicate over stdin/stdout.
    Stdio {
        command: PathBuf,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    /// Connect to a remote MCP server over Streamable HTTP (MCP spec 2025-03-26).
    StreamableHttp {
        endpoint: reqwest::Url,
        headers: HeaderMap,
    },
}
