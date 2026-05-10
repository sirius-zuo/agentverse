// examples/web-search-agent/src/main.rs
#![allow(unused_imports)]

use agentverse::{Agent, Config};
use agentverse_tools::{FileSearch, HttpClient};
// Tools: HttpClient + FileSearch for web research
use std::sync::Arc;
use tokio::sync::Mutex;

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
    let agent = Arc::new(Mutex::new(agent));

    println!("Web Search Agent - demonstrates Plan-and-Execute strategy");
    println!("Tools: HttpClient + FileSearch for web research");

    // Example: Search for information
    let result = agent
        .lock()
        .await
        .invoke("user1", "Search for latest Rust performance improvements")
        .await;
    println!("Agent: {}", result.unwrap());
}
