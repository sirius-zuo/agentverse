use agentverse_integration::whatsapp::WhatsAppConnector;
use agentverse_integration::{Connector, InputConnector, OutputConnector};

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
