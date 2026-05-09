// examples/code-review-agent/src/main.rs
#![allow(unused_imports)]

use agentverse::{Agent, Config};
use agentverse_tools::{Calculator, FileSearch};
// Tools: FileSearch + Calculator for code analysis
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let config = Config {
        model_api_key: std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
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
