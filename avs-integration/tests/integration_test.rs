// Smoke test: all public types are importable and the module compiles cleanly.
use agentverse_integration::{
    Connector, ConnectorError, Event, GithubConnector, Integration, IntegrationError, InvokerError,
    SlackConnector, WhatsAppConnector,
};

#[test]
fn all_public_types_accessible() {
    let _ = std::any::TypeId::of::<Event>();
    let _ = std::any::TypeId::of::<ConnectorError>();
    let _ = std::any::TypeId::of::<InvokerError>();
    let _ = std::any::TypeId::of::<IntegrationError>();
    let _ = std::any::TypeId::of::<Integration>();
}

#[test]
fn connector_names() {
    assert_eq!(SlackConnector::new("t", "s", 3000).name(), "slack");
    assert_eq!(GithubConnector::new("t", "s", 3001).name(), "github");
    assert_eq!(WhatsAppConnector::new("k", "p").name(), "whatsapp");
}
