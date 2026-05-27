use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::McpError;
use crate::transport::McpTransport;

#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

impl JsonRpcRequest {
    fn new(method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: Uuid::new_v4().to_string(),
            method: method.to_string(),
            params,
        }
    }
}

enum InnerClient {
    Http(reqwest::Client, reqwest::Url, reqwest::header::HeaderMap),
    Stdio {
        stdin: Mutex<ChildStdin>,
        stdout: Mutex<BufReader<ChildStdout>>,
    },
}

pub struct McpClient {
    inner: InnerClient,
}

impl McpClient {
    /// Connect and perform the MCP initialization handshake.
    pub async fn connect(transport: McpTransport) -> Result<Arc<Self>, McpError> {
        let client = match transport {
            McpTransport::StreamableHttp { endpoint, headers } => Arc::new(McpClient {
                inner: InnerClient::Http(reqwest::Client::new(), endpoint, headers),
            }),
            McpTransport::Stdio { command, args, env } => {
                let mut cmd = tokio::process::Command::new(&command);
                cmd.args(&args)
                    .envs(&env)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null());
                let mut child = cmd.spawn().map_err(|e| {
                    McpError::Connection(format!("Failed to spawn {:?}: {e}", command))
                })?;
                let stdin = child.stdin.take().unwrap();
                let stdout = BufReader::new(child.stdout.take().unwrap());
                Arc::new(McpClient {
                    inner: InnerClient::Stdio {
                        stdin: Mutex::new(stdin),
                        stdout: Mutex::new(stdout),
                    },
                })
            }
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&self) -> Result<(), McpError> {
        let req = JsonRpcRequest::new(
            "initialize",
            Some(json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "agentverse", "version": env!("CARGO_PKG_VERSION") }
            })),
        );
        let resp = self.send(req).await?;
        if resp.get("error").is_some() {
            return Err(McpError::Initialization(
                resp["error"]["message"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            ));
        }
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        self.send_notification(notif).await?;
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        let req = JsonRpcRequest::new("tools/list", Some(json!({})));
        let resp = self.send(req).await?;
        if let Some(err) = resp.get("error") {
            return Err(McpError::Protocol(
                err["message"].as_str().unwrap_or("").to_string(),
            ));
        }
        let tools = resp["result"]["tools"]
            .as_array()
            .ok_or_else(|| McpError::Parse("No tools array".into()))?;
        Ok(tools
            .iter()
            .map(|t| McpToolInfo {
                name: t["name"].as_str().unwrap_or("").to_string(),
                description: t["description"].as_str().unwrap_or("").to_string(),
                input_schema: t["inputSchema"].clone(),
            })
            .collect())
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, McpError> {
        let req = JsonRpcRequest::new(
            "tools/call",
            Some(json!({ "name": name, "arguments": args })),
        );
        let resp = self.send(req).await?;
        if let Some(err) = resp.get("error") {
            return Err(McpError::ToolCall(
                err["message"].as_str().unwrap_or("").to_string(),
            ));
        }
        let content = resp["result"]["content"]
            .as_array()
            .ok_or_else(|| McpError::Parse("No content array".into()))?;
        let text = content
            .iter()
            .find_map(|c| c["text"].as_str())
            .ok_or_else(|| McpError::Parse("No text content".into()))?;
        Ok(Value::String(text.to_string()))
    }

    async fn send(&self, req: JsonRpcRequest) -> Result<Value, McpError> {
        match &self.inner {
            InnerClient::Http(client, endpoint, headers) => {
                let mut builder = client.post(endpoint.clone());
                for (k, v) in headers.iter() {
                    builder = builder.header(k, v);
                }
                let resp = builder
                    .json(&req)
                    .send()
                    .await
                    .map_err(|e| McpError::Connection(e.to_string()))?;
                let ct = resp
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                if ct.contains("text/event-stream") {
                    let text = resp
                        .text()
                        .await
                        .map_err(|e| McpError::Parse(e.to_string()))?;
                    let data_line = text
                        .lines()
                        .find(|l| l.starts_with("data: "))
                        .ok_or_else(|| McpError::Parse("No SSE data line".into()))?;
                    serde_json::from_str(&data_line["data: ".len()..])
                        .map_err(|e| McpError::Parse(e.to_string()))
                } else {
                    resp.json::<Value>()
                        .await
                        .map_err(|e| McpError::Parse(e.to_string()))
                }
            }
            InnerClient::Stdio { stdin, stdout } => {
                let mut line = serde_json::to_string(&req).unwrap();
                line.push('\n');
                stdin
                    .lock()
                    .await
                    .write_all(line.as_bytes())
                    .await
                    .map_err(|e| McpError::Connection(e.to_string()))?;
                let mut response_line = String::new();
                stdout
                    .lock()
                    .await
                    .read_line(&mut response_line)
                    .await
                    .map_err(|e| McpError::Connection(e.to_string()))?;
                serde_json::from_str(response_line.trim())
                    .map_err(|e| McpError::Parse(e.to_string()))
            }
        }
    }

    async fn send_notification(&self, notif: Value) -> Result<(), McpError> {
        match &self.inner {
            InnerClient::Http(client, endpoint, headers) => {
                let mut builder = client.post(endpoint.clone());
                for (k, v) in headers.iter() {
                    builder = builder.header(k, v);
                }
                builder
                    .json(&notif)
                    .send()
                    .await
                    .map_err(|e| McpError::Connection(e.to_string()))?;
            }
            InnerClient::Stdio { stdin, .. } => {
                let mut line = serde_json::to_string(&notif).unwrap();
                line.push('\n');
                stdin
                    .lock()
                    .await
                    .write_all(line.as_bytes())
                    .await
                    .map_err(|e| McpError::Connection(e.to_string()))?;
            }
        }
        Ok(())
    }
}

impl McpClient {
    /// Creates a non-connected client for unit testing adapter construction.
    /// Not intended for production use.
    pub fn new_disconnected_for_test() -> Self {
        McpClient {
            inner: InnerClient::Http(
                reqwest::Client::new(),
                "http://localhost:0/mcp".parse().unwrap(),
                Default::default(),
            ),
        }
    }
}
