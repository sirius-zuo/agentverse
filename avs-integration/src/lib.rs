// avs-integration/src/lib.rs
pub mod adapter;
pub mod slack;
pub mod webhook;

pub use adapter::IntegrationAdapter;

pub mod error;
pub use error::{ConnectorError, IntegrationError, InvokerError};
pub use slack::SlackAdapter;
pub use webhook::{handle_webhook, WebhookAdapter, WebhookRequest, WebhookResponse, WebhookState};

pub mod event;
pub use event::Event;

pub mod connector;
pub use connector::{Connector, InputConnector, OutputConnector};

pub mod invoker;
pub use invoker::{AgentInvoker, StrategyInvoker};

pub mod integration;
pub use integration::Integration;
