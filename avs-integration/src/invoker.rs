use super::error::InvokerError;
use super::event::Event;
use agentverse::RunStrategy;
use async_trait::async_trait;
use tokio::sync::Mutex;

/// Clean boundary between the integration layer and agent logic.
///
/// The integration layer only knows about Events — no strategy types, tools, or prompts.
#[async_trait]
pub trait AgentInvoker: Send + Sync {
    async fn invoke(&self, event: Event) -> Result<Event, InvokerError>;
}

/// Bridges any `S: RunStrategy` to the `AgentInvoker` interface.
pub struct StrategyInvoker<S: RunStrategy> {
    strategy: Mutex<S>,
}

impl<S: RunStrategy> StrategyInvoker<S> {
    pub fn new(strategy: S) -> Self {
        Self {
            strategy: Mutex::new(strategy),
        }
    }
}

#[async_trait]
impl<S: RunStrategy + Send + 'static> AgentInvoker for StrategyInvoker<S> {
    async fn invoke(&self, event: Event) -> Result<Event, InvokerError> {
        let mut s = self.strategy.lock().await;
        let text = s
            .process(event.text.clone())
            .await
            .map_err(|e| InvokerError::Agent(e.to_string()))?;
        Ok(Event { text, ..event })
    }
}
