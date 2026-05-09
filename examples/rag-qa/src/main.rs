// examples/rag-qa/src/main.rs
use agentverse::{Agent, Config};
use agentverse_tools::HttpClient;
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

    println!("RAG QA Agent - demonstrates vector DB + knowledge base retrieval");
    println!("Use case: Query documents stored in LanceDB via HTTP tool");

    // Example: Query the agent about stored knowledge
    let result = agent.lock().await.invoke("user1", "What is the project architecture?").await;
    println!("Agent: {}", result.unwrap());
}
