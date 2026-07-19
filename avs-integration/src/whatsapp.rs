use super::connector::{Connector, InputConnector, OutputConnector};
use super::error::ConnectorError;
use super::event::Event;
use async_trait::async_trait;

/// Deprecated WhatsApp connector stub.
///
/// This API is not production-ready and returns a typed not-implemented
/// [`ConnectorError::Connection`] from its input and output operations.
///
/// Production implementation would use the WhatsApp Business Cloud API:
/// - Input: receive messages via webhook (similar to Slack/GitHub)
/// - Output: send messages via POST to the messages endpoint
#[deprecated(note = "stub connector; not production-ready")]
pub struct WhatsAppConnector {
    _api_key: String,
    _phone_number_id: String,
}

#[allow(deprecated)]
impl WhatsAppConnector {
    pub fn new(api_key: &str, phone_number_id: &str) -> Self {
        Self {
            _api_key: api_key.to_string(),
            _phone_number_id: phone_number_id.to_string(),
        }
    }
}

#[async_trait]
#[allow(deprecated)]
impl Connector for WhatsAppConnector {
    fn name(&self) -> &str {
        "whatsapp"
    }
}

#[async_trait]
#[allow(deprecated)]
impl InputConnector for WhatsAppConnector {
    async fn receive(&self) -> Result<Event, ConnectorError> {
        Err(ConnectorError::Connection(
            "WhatsAppConnector is not yet implemented".to_string(),
        ))
    }
}

#[async_trait]
#[allow(deprecated)]
impl OutputConnector for WhatsAppConnector {
    async fn send(&self, _event: Event) -> Result<(), ConnectorError> {
        Err(ConnectorError::Connection(
            "WhatsAppConnector is not yet implemented".to_string(),
        ))
    }
}
