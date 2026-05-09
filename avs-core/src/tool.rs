use serde_json::Value;

use crate::error::ToolError;

pub type ToolResult = Result<Value, ToolError>;

pub trait SyncTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn execute(&self, args: Value) -> ToolResult;
}

#[async_trait::async_trait]
pub trait AsyncTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    async fn execute(&self, args: Value) -> ToolResult;
}
