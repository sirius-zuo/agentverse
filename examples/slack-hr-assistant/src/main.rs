// examples/slack-hr-assistant/src/main.rs
use agentverse::{Agent, Config};
use agentverse_integration::{IntegrationAdapter, SlackAdapter};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let prompts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prompts");

    let config = Config {
        model_api_key: std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
        prompts_dir: Some(prompts_dir.to_string_lossy().to_string()),
        system_prompt: None,
    };

    let agent = Agent::from_config(config).unwrap();
    let agent = Arc::new(Mutex::new(agent));

    let adapter = SlackAdapter::new(
        agent,
        &std::env::var("SLACK_BOT_TOKEN").expect("SLACK_BOT_TOKEN not set"),
        &std::env::var("SLACK_SIGNING_SECRET").expect("SLACK_SIGNING_SECRET not set"),
        3000,
    );

    adapter
        .start()
        .await
        .expect("Failed to start Slack adapter");
}
