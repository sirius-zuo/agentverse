use crate::error::AgentError;
use crate::memory::Message;

#[async_trait::async_trait]
pub trait RunStrategy: Send + Sync {
    async fn run(&self, messages: Vec<Message>) -> Result<String, AgentError>;

    /// Run the strategy using only the named tools (subset of the registry).
    /// Default implementation ignores `active_tool_names` and calls `run`.
    /// Strategies that want per-call tool filtering should override this.
    async fn run_with_active_tools(
        &self,
        messages: Vec<Message>,
        _active_tool_names: &[String],
    ) -> Result<String, AgentError> {
        self.run(messages).await
    }
}
