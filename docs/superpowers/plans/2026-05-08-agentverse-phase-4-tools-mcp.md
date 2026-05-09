# Phase 4: Tools + MCP

> **Goal:** Implement built-in tools and MCP client for external tool access.
> **Dependencies:** Phase 1 (avs-core) must be complete
> **Parallel:** avs-tools and avs-mcp can develop in parallel

---

## Overview

Two paths for tools:
1. **Built-in tools** — Rust-native, compile-time type-safe parameters
2. **MCP tools** — External tools via MCP protocol, runtime-dynamic parameters

```
ToolRegistry (in avs-core, extended here)
    ├── Built-in tools: FileSearch, HttpClient, Calculator, DateTime
    └── MCP tools: dynamic registration via MCP protocol
```

## File Structure

```
avs-tools/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── registry.rs     # ToolRegistry: static + dynamic registration
│   ├── file_search.rs  # FileSearch tool
│   ├── http_client.rs  # HttpClient tool
│   ├── calculator.rs   # Calculator tool
│   └── datetime.rs     # DateTime tool
│
avs-mcp/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── client.rs       # MCP client (SSE transport)
│   └── tools.rs        # MCP tools → SyncTool adapter
```

---

## Task 1: avs-tools — Built-in tools + ToolRegistry

**Files:**
- Create: `avs-tools/Cargo.toml`
- Create: `avs-tools/src/lib.rs`
- Create: `avs-tools/src/registry.rs`
- Create: `avs-tools/src/file_search.rs`
- Create: `avs-tools/src/http_client.rs`
- Create: `avs-tools/src/calculator.rs`
- Create: `avs-tools/src/datetime.rs`
- Create: `avs-tools/tests/tools_test.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-tools"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
glob = "0.3"
chrono.workspace = true
```

- [ ] **Step 2: registry.rs — ToolRegistry**

```rust
// avs-tools/src/registry.rs
use agentverse::{SyncTool, ToolResult};
use std::collections::HashMap;

/// Registry of available tools.
/// Supports static registration (built-in) and dynamic registration (MCP).
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn SyncTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool by name.
    pub fn register(&mut self, tool: impl SyncTool + 'static) {
        self.tools.insert(tool.name().to_string(), Box::new(tool));
    }

    /// Register multiple tools.
    pub fn register_many(&mut self, tools: Vec<Box<dyn SyncTool>>) {
        for tool in tools {
            self.register(tool);
        }
    }

    /// Execute a tool by name.
    pub fn execute(&self, tool_name: &str, args: serde_json::Value) -> ToolResult {
        let tool = self.tools.get(tool_name)
            .ok_or_else(|| agentverse::ToolError::NotFound(tool_name.to_string()))?;
        tool.execute(args)
    }

    /// Get all registered tool names.
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Check if a tool exists.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: file_search.rs**

```rust
// avs-tools/src/file_search.rs
use agentverse::{SyncTool, ToolResult};
use glob::glob;
use serde_json::{json, Value};

/// Search files by pattern.
pub struct FileSearch;

impl SyncTool for FileSearch {
    fn name(&self) -> &str {
        "file_search"
    }

    fn description(&self) -> &str {
        "Search for files matching a pattern in a directory"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to search in"
                },
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g., '*.txt', '**/*.rs')"
                }
            },
            "required": ["path", "pattern"]
        })
    }

    fn execute(&self, args: Value) -> ToolResult {
        let path = args["path"].as_str().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing or invalid 'path' parameter".to_string())
        })?;

        let pattern = args["pattern"].as_str().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing or invalid 'pattern' parameter".to_string())
        })?;

        let full_pattern = format!("{}/{}", path, pattern);
        let matches: Vec<String> = glob(&full_pattern)
            .map_err(|e| agentverse::ToolError::Execution(e.to_string()))?
            .filter_map(|entry| entry.ok())
            .filter_map(|p| p.to_str().map(String::from))
            .collect();

        Ok(json!({
            "matches": matches,
            "count": matches.len()
        }))
    }
}
```

- [ ] **Step 4: http_client.rs**

```rust
// avs-tools/src/http_client.rs
use agentverse::{SyncTool, ToolResult};
use reqwest::blocking::Client;
use serde_json::{json, Value};

/// HTTP client tool for making REST API calls.
pub struct HttpClient;

impl SyncTool for HttpClient {
    fn name(&self) -> &str {
        "http_client"
    }

    fn description(&self) -> &str {
        "Make HTTP requests (GET, POST, PUT, DELETE)"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "DELETE"],
                    "description": "HTTP method"
                },
                "url": {
                    "type": "string",
                    "description": "Request URL"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional request headers"
                },
                "body": {
                    "type": "string",
                    "description": "Optional request body (for POST/PUT)"
                }
            },
            "required": ["method", "url"]
        })
    }

    fn execute(&self, args: Value) -> ToolResult {
        let method = args["method"].as_str().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing 'method' parameter".to_string())
        })?;

        let url = args["url"].as_str().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing 'url' parameter".to_string())
        })?;

        let client = Client::new();
        let request = match method.to_uppercase().as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            other => return Err(agentverse::ToolError::Execution(format!(
                "Unsupported method: {}", other
            ))),
        };

        let request = if let Some(headers) = args["headers"].as_object() {
            let mut req = request;
            for (k, v) in headers {
                if let Some(v_str) = v.as_str() {
                    req = req.header(k, v_str);
                }
            }
            req
        } else {
            request
        };

        let response = request.send()
            .map_err(|e| agentverse::ToolError::Execution(e.to_string()))?;

        let status = response.status().as_u16();
        let body = response.text()
            .map_err(|e| agentverse::ToolError::Execution(e.to_string()))?;

        Ok(json!({
            "status": status,
            "body": body
        }))
    }
}
```

- [ ] **Step 5: calculator.rs**

```rust
// avs-tools/src/calculator.rs
use agentverse::{SyncTool, ToolResult};
use serde_json::{json, Value};

/// Simple calculator tool for arithmetic operations.
pub struct Calculator;

impl SyncTool for Calculator {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Perform arithmetic calculations: add, subtract, multiply, divide"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["add", "subtract", "multiply", "divide"],
                    "description": "Arithmetic operation"
                },
                "a": {
                    "type": "number",
                    "description": "First operand"
                },
                "b": {
                    "type": "number",
                    "description": "Second operand"
                }
            },
            "required": ["operation", "a", "b"]
        })
    }

    fn execute(&self, args: Value) -> ToolResult {
        let op = args["operation"].as_str().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing 'operation'".to_string())
        })?;

        let a = args["a"].as_f64().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing or invalid 'a'".to_string())
        })?;

        let b = args["b"].as_f64().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing or invalid 'b'".to_string())
        })?;

        let result = match op {
            "add" => a + b,
            "subtract" => a - b,
            "multiply" => a * b,
            "divide" => {
                if b == 0.0 {
                    return Err(agentverse::ToolError::Execution("Division by zero".to_string()));
                }
                a / b
            }
            other => return Err(agentverse::ToolError::Execution(format!(
                "Unknown operation: {}", other
            ))),
        };

        Ok(json!({ "result": result }))
    }
}
```

- [ ] **Step 6: datetime.rs**

```rust
// avs-tools/src/datetime.rs
use agentverse::{SyncTool, ToolResult};
use chrono::Utc;
use serde_json::{json, Value};

/// Current date and time tool.
pub struct DateTimeTool;

impl SyncTool for DateTimeTool {
    fn name(&self) -> &str {
        "datetime"
    }

    fn description(&self) -> &str {
        "Get the current date and time in UTC"
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn execute(&self, _args: Value) -> ToolResult {
        let now = Utc::now();
        Ok(json!({
            "utc": now.to_rfc3339(),
            "unix_timestamp": now.timestamp(),
            "date": now.format("%Y-%m-%d").to_string(),
            "time": now.format("%H:%M:%S").to_string()
        }))
    }
}
```

- [ ] **Step 7: lib.rs**

```rust
// avs-tools/src/lib.rs
pub mod calculator;
pub mod datetime;
pub mod file_search;
pub mod http_client;
pub mod registry;

pub use calculator::Calculator;
pub use datetime::DateTimeTool;
pub use file_search::FileSearch;
pub use http_client::HttpClient;
pub use registry::ToolRegistry;
```

- [ ] **Step 8: Tests + verify + commit**

Run: `cargo check -p agentverse-tools`
Run: `cargo test -p agentverse-tools`
Commit: `git add avs-tools/ && git commit -m "feat: add built-in tools (FileSearch, HttpClient, Calculator, DateTime)"`

---

## Task 2: avs-mcp — MCP Client + Tool Adapter

**Files:**
- Create: `avs-mcp/Cargo.toml`
- Create: `avs-mcp/src/lib.rs`
- Create: `avs-mcp/src/client.rs`
- Create: `avs-mcp/src/tools.rs`
- Create: `avs-mcp/tests/mcp_test.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-mcp"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
uuid.workspace = true
```

- [ ] **Step 2: client.rs — MCP client (SSE transport)**

```rust
// avs-mcp/src/client.rs
use serde_json::Value;
use uuid::Uuid;

/// MCP Client for connecting to MCP servers via SSE transport.
/// MVP supports only SSE (not stdio).
pub struct McpClient {
    server_url: String,
    client: reqwest::Client,
}

/// An MCP tool definition from the server
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, serde::Serialize)]
struct McpRequest {
    id: String,
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

impl McpClient {
    pub fn new(server_url: &str) -> Self {
        Self {
            server_url: server_url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Initialize connection with the MCP server.
    pub async fn initialize(&self) -> Result<(), McpError> {
        let request = McpRequest {
            id: Uuid::new_v4().to_string(),
            jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "agentverse",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        };

        let response = self.client
            .post(&format!("{}/message", self.server_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

        if !response.status().is_success() {
            return Err(McpError::Initialization(response.status().as_u16()));
        }

        Ok(())
    }

    /// List available tools from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        let request = McpRequest {
            id: Uuid::new_v4().to_string(),
            jsonrpc: "2.0".to_string(),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = self.client
            .post(&format!("{}/message", self.server_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

        let body: Value = response.json().await
            .map_err(|e| McpError::Parse(e.to_string()))?;

        let tools = body["result"]["tools"]
            .as_array()
            .ok_or_else(|| McpError::Parse("No tools array in response".to_string()))?;

        let mut result = Vec::new();
        for tool in tools {
            result.push(McpToolInfo {
                name: tool["name"].as_str().unwrap_or("").to_string(),
                description: tool["description"].as_str().unwrap_or("").to_string(),
                parameters: tool["inputSchema"].clone(),
            });
        }

        Ok(result)
    }

    /// Call an MCP tool.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, McpError> {
        let request = McpRequest {
            id: Uuid::new_v4().to_string(),
            jsonrpc: "2.0".to_string(),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": name,
                "arguments": args
            })),
        };

        let response = self.client
            .post(&format!("{}/message", self.server_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

        let body: Value = response.json().await
            .map_err(|e| McpError::Parse(e.to_string()))?;

        if let Some(error) = body.get("error") {
            return Err(McpError::ToolCall(error["message"].as_str()
                .unwrap_or("Unknown error").to_string()));
        }

        Ok(body["result"]["content"][0]["text"].clone())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum McpError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Initialization failed with status: {}", 0)]
    Initialization(u16),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Tool call error: {0}")]
    ToolCall(String),
}
```

- [ ] **Step 3: tools.rs — MCP tools → SyncTool adapter**

```rust
// avs-mcp/src/tools.rs
use super::client::McpClient;
use agentverse::{SyncTool, ToolResult};
use serde_json::Value;
use std::sync::Arc;

/// Adapter that wraps an MCP tool as a SyncTool.
/// Executes MCP tools via the client.
pub struct McpToolAdapter {
    name: String,
    description: String,
    parameters: Value,
    client: Arc<McpClient>,
}

impl McpToolAdapter {
    pub fn new(name: String, description: String, parameters: Value, client: Arc<McpClient>) -> Self {
        Self {
            name,
            description,
            parameters,
            client,
        }
    }
}

impl SyncTool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    fn execute(&self, args: Value) -> ToolResult {
        // MCP execution is async, so we spawn a blocking task
        // In production, use a runtime handle
        let client = Arc::clone(&self.client);
        let name = self.name.clone();
        let args = args.clone();

        // Use tokio::runtime::Handle to run async in sync context
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let rt = handle;
                // Block on the async call — this is a limitation of the
                // SyncTool trait. In production, consider using AsyncTool
                // for MCP tools.
                rt.block_on(async {
                    client.call_tool(&name, args).await
                        .map_err(|e| agentverse::ToolError::Execution(e.to_string()))
                })
            }
            Err(_) => Err(agentverse::ToolError::Execution(
                "No tokio runtime available for MCP tool execution".to_string()
            )),
        }
    }
}
```

- [ ] **Step 4: lib.rs**

```rust
// avs-mcp/src/lib.rs
pub mod client;
pub mod tools;

pub use client::{McpClient, McpError, McpToolInfo};
pub use tools::McpToolAdapter;
```

- [ ] **Step 5: Tests + verify + commit**

Run: `cargo check -p agentverse-mcp`
Run: `cargo test -p agentverse-mcp`
Commit: `git add avs-mcp/ && git commit -m "feat: add MCP client and tool adapter"`

---

## Phase 4 Acceptance Criteria

- [ ] `ToolRegistry` registers and executes built-in tools
- [ ] `FileSearch`, `HttpClient`, `Calculator`, `DateTime` all work
- [ ] `McpClient` can initialize and list tools (mock server)
- [ ] `McpToolAdapter` wraps MCP tools as `SyncTool`
- [ ] Clippy passes for both crates

## Parallel Execution Notes

- `avs-tools` and `avs-mcp` are **independent** — can be parallelized
- Both depend only on `avs-core`

## Estimated Effort

~6-8 hours total. With parallelization: ~3-4 hours.
