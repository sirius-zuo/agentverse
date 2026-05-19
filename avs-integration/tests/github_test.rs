use agentverse_integration::github::GithubConnector;
use agentverse_integration::{Connector, InputConnector, OutputConnector};

#[test]
fn github_connector_name() {
    let c = GithubConnector::new("ghp_token", "webhook_secret", 3200);
    assert_eq!(c.name(), "github");
}

#[test]
fn github_connector_implements_input_and_output() {
    fn assert_input<T: InputConnector>() {}
    fn assert_output<T: OutputConnector>() {}
    assert_input::<GithubConnector>();
    assert_output::<GithubConnector>();
}
