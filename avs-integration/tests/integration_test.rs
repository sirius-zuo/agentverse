// avs-integration/tests/integration_test.rs
// Smoke test: verify all public types are importable after the redesign.
#[allow(deprecated)]
use agentverse_integration::WhatsAppConnector;
use agentverse_integration::{
    Connector, ConnectorError, Event, GithubConnector, IntegrationConfig, IntegrationError,
    IntegrationRuntime, SlackConnector,
};

#[test]
fn all_public_types_accessible() {
    let _ = std::any::TypeId::of::<Event>();
    let _ = std::any::TypeId::of::<ConnectorError>();
    let _ = std::any::TypeId::of::<IntegrationError>();
    let _ = std::any::TypeId::of::<IntegrationRuntime>();
    let _ = std::any::TypeId::of::<IntegrationConfig>();
}

#[test]
#[allow(deprecated)]
fn connector_names() {
    assert_eq!(SlackConnector::new("t", "s", 3000).name(), "slack");
    assert_eq!(GithubConnector::new("t", "s", 3001).name(), "github");
    assert_eq!(WhatsAppConnector::new("k", "p").name(), "whatsapp");
}
