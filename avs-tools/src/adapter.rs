use agentverse::{AsyncTool, SyncTool, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

/// Wraps any CPU-bound [`SyncTool`] as an [`AsyncTool`].
///
/// The inner tool's `execute` is called directly on the async thread, which is
/// safe for tools that only do CPU work (e.g. `Calculator`, `DateTimeTool`,
/// `FileSearch`). Do **not** use this for tools that perform blocking I/O —
/// those should implement `AsyncTool` natively (see `HttpClient`, `ShellTool`).
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
