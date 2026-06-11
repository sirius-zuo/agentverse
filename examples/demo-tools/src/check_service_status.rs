use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckServiceStatusArgs {
    /// Service or region to check (e.g. "api", "us-east-1")
    pub service: String,
}

pub struct CheckServiceStatus;

#[async_trait::async_trait]
impl Tool for CheckServiceStatus {
    type Args = CheckServiceStatusArgs;
    fn name(&self) -> &str {
        "check_service_status"
    }
    fn description(&self) -> &str {
        "Check the current operational status of a service or region. \
         Returns status (operational/degraded/outage) and incident details."
    }
    async fn execute(&self, args: CheckServiceStatusArgs) -> ToolResult {
        Ok(json!({
            "service":              args.service,
            "status":               "degraded",
            "region":               "us-east-1",
            "message":              "Elevated API latency observed since 2026-06-11T08:30:00Z",
            "estimated_resolution": "2026-06-11T14:00:00Z"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn check_service_status_returns_degraded() {
        let tool = CheckServiceStatus;
        let result = tool
            .execute(CheckServiceStatusArgs {
                service: "api".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(result["status"], "degraded");
        assert_eq!(result["service"], "api");
    }
}
