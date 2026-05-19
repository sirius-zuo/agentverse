// examples/web-search-agent/src/main.rs
//
// Plan-and-Execute agent with HttpClient — demonstrates multi-step web research.
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-web-search-agent

use agentverse::{OpenAICompatible, PromptConfig, PromptRegistry};
use agentverse_memory::SimpleMemory;
use agentverse_plan::PlanStrategy;
use agentverse_tools::{HttpClient, ToolRegistry};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    println!("Web Search Agent — model: {} @ {}", model_name, base_url);

    let model = Arc::new(OpenAICompatible::new(&base_url, &model_name, &api_key));
    let registry = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );
    let memory = Arc::new(Mutex::new(SimpleMemory::new(50)));
    let mut tools = ToolRegistry::new();
    tools.register(HttpClient);
    let mut agent = PlanStrategy::new(model, registry, tools, memory, 10);

    let question = "Fetch https://httpbin.org/get and https://httpbin.org/uuid, \
                    then summarize what each endpoint returned."
        .to_string();
    println!("> {}", question);

    match agent.run(question).await {
        Ok(answer) => println!("\nAgent: {}", answer),
        Err(e) => eprintln!("Error: {}", e),
    }
}
