use agentverse::{AsyncTool, ToolError, ToolResult};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::LazyLock;
use std::time::Duration;
use url::Url;

static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client")
});

/// HTTP client tool for making REST API calls.
pub struct HttpClient;

#[async_trait]
impl AsyncTool for HttpClient {
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

    async fn execute(&self, args: Value) -> ToolResult {
        let method = args["method"].as_str().ok_or_else(|| {
            ToolError::Execution("Missing 'method' parameter".to_string())
        })?;

        let url = args["url"].as_str().ok_or_else(|| {
            ToolError::Execution("Missing 'url' parameter".to_string())
        })?;

        let parsed = Url::parse(url)
            .map_err(|e| ToolError::Execution(format!("Invalid URL: {e}")))?;
        if !"http".eq_ignore_ascii_case(parsed.scheme())
            && !"https".eq_ignore_ascii_case(parsed.scheme())
        {
            return Err(ToolError::Execution(
                "Only http and https URLs are allowed".to_string(),
            ));
        }

        let mut request = match method.to_uppercase().as_str() {
            "GET" => HTTP_CLIENT.get(url),
            "POST" => HTTP_CLIENT.post(url),
            "PUT" => HTTP_CLIENT.put(url),
            "DELETE" => HTTP_CLIENT.delete(url),
            other => {
                return Err(ToolError::Execution(format!("Unsupported method: {other}")))
            }
        };

        if let Some(headers) = args["headers"].as_object() {
            for (k, v) in headers {
                if let Some(v_str) = v.as_str() {
                    request = request.header(k, v_str);
                }
            }
        }

        if let Some(body) = args["body"].as_str() {
            request = request.body(body.to_string());
        }

        let response = request
            .send()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(json!({ "status": status, "body": body }))
    }
}
