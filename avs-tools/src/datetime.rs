use agentverse::{Tool, ToolResult};
use chrono::Utc;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
pub struct DateTimeArgs {}

pub struct DateTimeTool;

#[async_trait::async_trait]
impl Tool for DateTimeTool {
    type Args = DateTimeArgs;

    fn name(&self) -> &str {
        "datetime"
    }

    fn description(&self) -> &str {
        "Get the current date and time in UTC"
    }

    async fn execute(&self, _args: DateTimeArgs) -> ToolResult {
        let now = Utc::now();
        Ok(json!({
            "utc": now.to_rfc3339(),
            "unix_timestamp": now.timestamp(),
            "date": now.format("%Y-%m-%d").to_string(),
            "time": now.format("%H:%M:%S").to_string()
        }))
    }
}
