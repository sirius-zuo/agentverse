#![allow(deprecated)]

use agentverse_integration::whatsapp::WhatsAppConnector;
use agentverse_integration::{Connector, ConnectorError, Event, InputConnector, OutputConnector};
use std::collections::HashMap;

#[test]
fn whatsapp_connector_name() {
    let c = WhatsAppConnector::new("api_key", "phone_number_id");
    assert_eq!(c.name(), "whatsapp");
}

#[test]
fn whatsapp_connector_implements_input_and_output() {
    fn assert_input<T: InputConnector>() {}
    fn assert_output<T: OutputConnector>() {}
    assert_input::<WhatsAppConnector>();
    assert_output::<WhatsAppConnector>();
}

#[tokio::test]
async fn whatsapp_receive_returns_typed_not_implemented_error() {
    let connector = WhatsAppConnector::new("api_key", "phone_number_id");

    let error = connector.receive().await.unwrap_err();

    assert!(matches!(
        error,
        ConnectorError::Connection(message) if message == "WhatsAppConnector is not yet implemented"
    ));
}

#[tokio::test]
async fn whatsapp_send_returns_typed_not_implemented_error() {
    let connector = WhatsAppConnector::new("api_key", "phone_number_id");
    let event = Event {
        id: uuid::Uuid::new_v4(),
        conversation_id: "conversation".to_string(),
        user_id: "user".to_string(),
        text: "hello".to_string(),
        metadata: HashMap::new(),
    };

    let error = connector.send(event).await.unwrap_err();

    assert!(matches!(
        error,
        ConnectorError::Connection(message) if message == "WhatsAppConnector is not yet implemented"
    ));
}
