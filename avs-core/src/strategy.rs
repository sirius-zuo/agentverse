use crate::error::AgentError;
use crate::hitl::HitlHook;
use crate::memory::Message;
use std::sync::Arc;

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

    /// Run with HITL support. Default delegates to run_with_active_tools (no HITL).
    /// ReActStrategy overrides this to intercept dangerous tool calls.
    async fn run_hitl(
        &self,
        messages: Vec<Message>,
        active_tool_names: &[String],
        _hook: Arc<dyn HitlHook>,
    ) -> Result<String, AgentError> {
        self.run_with_active_tools(messages, active_tool_names).await
    }
}
