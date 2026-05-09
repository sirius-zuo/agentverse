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
