use agentverse::{Tool, ToolError, ToolResult};
use reqwest::Client;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;
use url::Url;

static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client")
});

#[derive(Deserialize, JsonSchema)]
pub struct HttpClientArgs {
    /// HTTP method (GET, POST, PUT, DELETE)
    pub method: String,
    /// Full URL including scheme (http/https only)
    pub url: String,
    /// Optional request headers as key-value pairs
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Optional request body (for POST/PUT)
    pub body: Option<Value>,
}

pub struct HttpClient;

#[async_trait::async_trait]
impl Tool for HttpClient {
    type Args = HttpClientArgs;

    fn name(&self) -> &str {
        "http_client"
    }

    fn description(&self) -> &str {
        "Make HTTP requests (GET, POST, PUT, DELETE)"
    }

    async fn execute(&self, args: HttpClientArgs) -> ToolResult {
        let parsed = Url::parse(&args.url)
            .map_err(|e| ToolError::Execution(format!("Invalid URL: {e}")))?;
        if !"http".eq_ignore_ascii_case(parsed.scheme())
            && !"https".eq_ignore_ascii_case(parsed.scheme())
        {
            return Err(ToolError::Execution(
                "Only http and https URLs are allowed".to_string(),
            ));
        }

        let mut request = match args.method.to_uppercase().as_str() {
            "GET" => HTTP_CLIENT.get(&args.url),
            "POST" => HTTP_CLIENT.post(&args.url),
            "PUT" => HTTP_CLIENT.put(&args.url),
            "DELETE" => HTTP_CLIENT.delete(&args.url),
            other => return Err(ToolError::Execution(format!("Unsupported method: {other}"))),
        };

        for (k, v) in &args.headers {
            request = request.header(k, v);
        }

        if let Some(body) = args.body {
            request = request.json(&body);
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
