// avs-integration/src/lib.rs
pub mod config;
pub mod connector;
pub mod console;
pub mod error;
pub mod event;
pub mod github;
pub mod runtime;
pub mod slack;
pub mod whatsapp;

pub use config::IntegrationConfig;
pub use connector::{Connector, InputConnector, OutputConnector};
pub use error::{ConnectorError, IntegrationError};
pub use event::Event;
pub use github::GithubConnector;
pub use runtime::IntegrationRuntime;
pub use slack::SlackConnector;
pub use whatsapp::WhatsAppConnector;
