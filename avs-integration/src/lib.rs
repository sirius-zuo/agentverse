// avs-integration/src/lib.rs
pub mod adapter;
pub mod slack;
pub mod webhook;

pub use adapter::{IntegrationAdapter, IntegrationError};
pub use slack::SlackAdapter;
pub use webhook::{WebhookAdapter, WebhookRequest, WebhookResponse};
