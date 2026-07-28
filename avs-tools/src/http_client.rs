use agentverse::{Tool, ToolError, ToolResult};
use reqwest::Client;
use schemars::JsonSchema;
use serde::Deserialize;
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HeaderPair {
    /// Header name
    pub key: String,
    /// Header value
    pub value: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct HttpClientArgs {
    /// HTTP method (GET, POST, PUT, DELETE)
    pub method: String,
    /// Full URL including scheme (http/https only)
    pub url: String,
    /// Optional request headers as key-value pairs
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
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
        let parsed =
            Url::parse(&args.url).map_err(|e| ToolError::Execution(format!("Invalid URL: {e}")))?;
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

        for pair in &args.headers {
            request = request.header(&pair.key, &pair.value);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_deserialize_headers_as_key_value_pairs() {
        let args: HttpClientArgs = serde_json::from_value(json!({
            "method": "GET",
            "url": "http://example.com",
            "headers": [{"key": "X-Test", "value": "1"}]
        }))
        .unwrap();
        assert_eq!(args.headers.len(), 1);
        assert_eq!(args.headers[0].key, "X-Test");
        assert_eq!(args.headers[0].value, "1");
    }

    #[test]
    fn args_default_to_empty_headers_when_absent() {
        let args: HttpClientArgs = serde_json::from_value(json!({
            "method": "GET",
            "url": "http://example.com"
        }))
        .unwrap();
        assert!(args.headers.is_empty());
    }
}
