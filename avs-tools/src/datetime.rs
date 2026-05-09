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
