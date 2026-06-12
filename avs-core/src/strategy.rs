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

    /// Run with HITL support.
    ///
    /// **WARNING — SECURITY RISK IF NOT OVERRIDDEN:** The default implementation
    /// ignores `hook` entirely and falls back to `run_with_active_tools`. Any
    /// strategy that does not override this method will execute all tool calls
    /// without HITL interception, even when the agent has a `HitlConfig` configured.
    ///
    /// Override this in any strategy that has a tool-execution loop.
    /// `ReActStrategy` provides the reference implementation.
    async fn run_hitl(
        &self,
        messages: Vec<Message>,
        active_tool_names: &[String],
        _hook: Arc<dyn HitlHook>,
    ) -> Result<String, AgentError> {
        self.run_with_active_tools(messages, active_tool_names)
            .await
    }
}
