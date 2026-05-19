use agentverse_integration::{ConnectorError, IntegrationError, InvokerError};

#[test]
fn connector_error_display() {
    let e = ConnectorError::Connection("timeout".to_string());
    assert_eq!(e.to_string(), "Connection failed: timeout");
}

#[test]
fn integration_error_output_display() {
    let e = IntegrationError::Output {
        connector: "slack".to_string(),
        source: ConnectorError::Auth("bad token".to_string()),
    };
    assert!(e.to_string().contains("slack"));
    assert!(e.to_string().contains("bad token"));
}
