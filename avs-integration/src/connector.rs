use super::error::ConnectorError;
use super::event::Event;
use async_trait::async_trait;
use std::sync::Arc;

/// Base trait for all platform connectors.
#[async_trait]
pub trait Connector: Send + Sync {
    fn name(&self) -> &str;

    /// Optional lifecycle hook — called by Integration before the run loop.
    async fn start(&self) -> Result<(), ConnectorError> {
        Ok(())
    }
}

/// A connector that can receive Events from a platform.
#[async_trait]
pub trait InputConnector: Connector {
    async fn receive(&self) -> Result<Event, ConnectorError>;
}

/// A connector that can send Events to a platform.
#[async_trait]
pub trait OutputConnector: Connector {
    async fn send(&self, event: Event) -> Result<(), ConnectorError>;
}

// Arc<T> blanket impls so a connector can be shared between input and output roles.
#[async_trait]
impl<T: Connector + Send + Sync> Connector for Arc<T> {
    fn name(&self) -> &str {
        (**self).name()
    }
}

#[async_trait]
impl<T: InputConnector + Send + Sync> InputConnector for Arc<T> {
    async fn receive(&self) -> Result<Event, ConnectorError> {
        (**self).receive().await
    }
}

#[async_trait]
impl<T: OutputConnector + Send + Sync> OutputConnector for Arc<T> {
    async fn send(&self, event: Event) -> Result<(), ConnectorError> {
        (**self).send(event).await
    }
}
