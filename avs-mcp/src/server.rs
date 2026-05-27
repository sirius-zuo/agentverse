use agentverse_tools::ToolRegistry;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Clone)]
struct ServerState {
    registry: Arc<ToolRegistry>,
}

pub struct McpServer {
    registry: Arc<ToolRegistry>,
    listener: Option<TcpListener>,
}

impl McpServer {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self {
            registry,
            listener: None,
        }
    }

    /// Bind to a random available port. Returns the port number.
    pub async fn bind_random_port(&mut self) -> Result<u16, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        self.listener = Some(listener);
        Ok(port)
    }

    /// Run the server. Blocks until shutdown.
    pub async fn run(self) {
        let state = ServerState {
            registry: self.registry,
        };
        let app = Router::new()
            .route("/mcp", post(handle_mcp))
            .with_state(state);
        let listener = self.listener.expect("call bind_random_port before run");
        axum::serve(listener, app).await.unwrap();
    }
}

async fn handle_mcp(
    State(state): State<ServerState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let method = body["method"].as_str().unwrap_or("");
    let id = body["id"].clone();

    let result = match method {
        "initialize" => json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "agentverse", "version": env!("CARGO_PKG_VERSION") }
            }
        }),
        "notifications/initialized" => {
            return (StatusCode::NO_CONTENT, Json(Value::Null));
        }
        "tools/list" => {
            let schemas = state.registry.schema();
            let tools: Vec<Value> = schemas
                .into_iter()
                .map(|s| {
                    json!({
                        "name": s["name"],
                        "description": s["description"],
                        "inputSchema": s["input_schema"],
                    })
                })
                .collect();
            json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": tools } })
        }
        "tools/call" => {
            let name = body["params"]["name"].as_str().unwrap_or("");
            let args = body["params"]["arguments"].clone();
            match state.registry.execute(name, args).await {
                Ok(v) => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "content": [{ "type": "text", "text": v.to_string() }] }
                }),
                Err(e) => json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32000, "message": e.to_string() }
                }),
            }
        }
        _ => json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {method}") }
        }),
    };
    (StatusCode::OK, Json(result))
}
