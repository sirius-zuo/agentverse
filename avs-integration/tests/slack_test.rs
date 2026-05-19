use agentverse_integration::slack::SlackConnector;
use agentverse_integration::{Connector, InputConnector, OutputConnector};

#[test]
fn slack_connector_name() {
    let c = SlackConnector::new("xoxb-test", "secret", 3100);
    assert_eq!(c.name(), "slack");
}

#[test]
fn slack_connector_implements_input_and_output() {
    fn assert_input<T: InputConnector>() {}
    fn assert_output<T: OutputConnector>() {}
    assert_input::<SlackConnector>();
    assert_output::<SlackConnector>();
}
