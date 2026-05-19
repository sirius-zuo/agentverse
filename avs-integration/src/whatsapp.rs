use super::connector::{Connector, InputConnector, OutputConnector};
use super::error::ConnectorError;
use super::event::Event;
use async_trait::async_trait;

/// WhatsApp connector — stub for future implementation.
///
/// Production implementation would use the WhatsApp Business Cloud API:
/// - Input: receive messages via webhook (similar to Slack/GitHub)
/// - Output: send messages via POST to the messages endpoint
pub struct WhatsAppConnector {
    _api_key: String,
    _phone_number_id: String,
}

impl WhatsAppConnector {
    pub fn new(api_key: &str, phone_number_id: &str) -> Self {
        Self {
            _api_key: api_key.to_string(),
            _phone_number_id: phone_number_id.to_string(),
        }
    }
}

#[async_trait]
impl Connector for WhatsAppConnector {
    fn name(&self) -> &str {
        "whatsapp"
    }
}

#[async_trait]
impl InputConnector for WhatsAppConnector {
    async fn receive(&self) -> Result<Event, ConnectorError> {
        Err(ConnectorError::Connection(
            "WhatsAppConnector is not yet implemented".to_string(),
        ))
    }
}

#[async_trait]
impl OutputConnector for WhatsAppConnector {
    async fn send(&self, _event: Event) -> Result<(), ConnectorError> {
        Err(ConnectorError::Connection(
            "WhatsAppConnector is not yet implemented".to_string(),
        ))
    }
}
