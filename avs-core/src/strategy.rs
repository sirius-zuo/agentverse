use crate::error::AgentError;

/// Uniform interface for all agent strategies.
///
/// The method is named `process` rather than `run` to avoid ambiguity with
/// `ReActStrategy::run()`, which returns `CycleResult` instead of `String`.
#[async_trait::async_trait]
pub trait RunStrategy: Send {
    async fn process(&mut self, input: String) -> Result<String, AgentError>;
}
