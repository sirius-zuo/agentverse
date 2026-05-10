// examples/hello-agent/src/main.rs
use agentverse::{Agent, Config};
use std::path::PathBuf;

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

    println!("Hello Agent - a simple friendly assistant");
    println!("Ask the agent anything:");
    println!("> Hello, what can you do?");
    let result = agent.invoke("user1", "Hello, what can you do?").await;
    println!("Agent: {}", result.unwrap());
}
