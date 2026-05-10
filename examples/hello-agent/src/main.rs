// examples/hello-agent/src/main.rs
use agentverse::{Agent, Config};

#[tokio::main]
async fn main() {
    let config = Config {
        model_api_key: std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };

    let agent = Agent::from_config(config).unwrap();

    println!("Ask the agent anything:");
    println!("> Hello, what can you do?");
    let result = agent.invoke("user1", "Hello, what can you do?").await;
    println!("Agent: {}", result.unwrap());
}
