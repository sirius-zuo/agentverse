// examples/web-search-agent/src/main.rs
#![allow(unused_imports)]

use agentverse::{Agent, Config, ProviderConfig};
use agentverse_tools::{FileSearch, HttpClient};
// Tools: HttpClient + FileSearch for web research
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let prompts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prompts");

    let config = Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4".to_string(),
            api_key: std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
            base_url: None,
        },
        max_messages: 50,
        tools: vec![],
        prompts_dir: Some(prompts_dir.to_string_lossy().to_string()),
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
