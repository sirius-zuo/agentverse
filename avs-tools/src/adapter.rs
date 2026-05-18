use agentverse::{AsyncTool, SyncTool, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

/// Wraps any `SyncTool` as an `AsyncTool` without modifying the inner tool.
pub struct SyncToolAdapter<T: SyncTool>(pub T);

#[async_trait]
impl<T: SyncTool + 'static> AsyncTool for SyncToolAdapter<T> {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn parameters(&self) -> Value {
        self.0.parameters()
    }

    async fn execute(&self, args: Value) -> ToolResult {
        self.0.execute(args)
    }
}
