//! Platform connectors and the example-backed [`IntegrationRuntime`].
//!
//! This crate is an incubator: [`IntegrationRuntime`] is maintained by the
//! integration tests and `example-slack-hr-assistant`, but is not an
//! `avs-agent` core runtime path.

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
pub use console::ConsoleConnector;
pub use error::{ConnectorError, IntegrationError};
pub use event::Event;
pub use github::GithubConnector;
pub use runtime::IntegrationRuntime;
pub use slack::SlackConnector;
#[allow(deprecated)]
pub use whatsapp::WhatsAppConnector;
