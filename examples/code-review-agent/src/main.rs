// examples/code-review-agent/src/main.rs
#![allow(unused_imports)]

use agentverse::{Agent, Config, ProviderConfig};
use agentverse_tools::{Calculator, FileSearch};
// Tools: FileSearch + Calculator for code analysis
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

    println!("Code Review Agent - demonstrates Hierarchical Planning");
    println!("Tools: FileSearch + Calculator for code analysis");

    // Example: Review code quality
    let result = agent
        .lock()
        .await
        .invoke("user1", "Review the codebase for security issues")
        .await;
    println!("Agent: {}", result.unwrap());
}
