use agentverse_integration::IntegrationConfig;

#[test]
fn parse_slack_config() {
    let toml_str = r#"
[integration]
input = "slack"
outputs = ["slack"]

[connector.slack]
port = 3000
bot_token_env = "SLACK_BOT_TOKEN"
signing_secret_env = "SLACK_SIGNING_SECRET"
"#;
    let config: IntegrationConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.integration.input, "slack");
    assert_eq!(config.integration.outputs, vec!["slack"]);
    let slack = config.connector.slack.unwrap();
    assert_eq!(slack.port, 3000);
    assert_eq!(slack.bot_token_env, "SLACK_BOT_TOKEN");
    assert_eq!(slack.signing_secret_env, "SLACK_SIGNING_SECRET");
}

#[test]
fn parse_multi_connector_config() {
    let toml_str = r#"
[integration]
input = "github"
outputs = ["github", "slack"]

[connector.slack]
port = 3000
bot_token_env = "SLACK_BOT_TOKEN"
signing_secret_env = "SLACK_SIGNING_SECRET"

[connector.github]
port = 3001
token_env = "GITHUB_TOKEN"
webhook_secret_env = "GITHUB_WEBHOOK_SECRET"
"#;
    let config: IntegrationConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.integration.input, "github");
    assert_eq!(config.integration.outputs, vec!["github", "slack"]);
    assert!(config.connector.slack.is_some());
    assert!(config.connector.github.is_some());
    let github = config.connector.github.unwrap();
    assert_eq!(github.port, 3001);
    assert_eq!(github.token_env, "GITHUB_TOKEN");
    assert_eq!(github.webhook_secret_env, "GITHUB_WEBHOOK_SECRET");
}

#[test]
fn parse_fails_on_missing_required_field() {
    // outputs is required
    let toml_str = r#"
[integration]
input = "slack"
"#;
    let result = toml::from_str::<IntegrationConfig>(toml_str);
    assert!(result.is_err());
}
