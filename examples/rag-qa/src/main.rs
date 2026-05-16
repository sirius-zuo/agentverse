// examples/rag-qa/src/main.rs
//
// Agent with a Calculator tool — exercises the tool-call loop.
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-rag-qa

use agentverse::{OpenAICompatible, PromptConfig, PromptRegistry, ShortTermMemory};
use agentverse_react::ReActStrategy;
use agentverse_tools::Calculator;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    println!("RAG QA Agent — model: {} @ {}", model_name, base_url);
    println!("Tool: Calculator");

    let model = Arc::new(OpenAICompatible::new(&base_url, &model_name, &api_key));
    let registry = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(
                concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string(),
            ),
            ..Default::default()
        })
        .expect("prompt config"),
    );
    let memory = Arc::new(Mutex::new(ShortTermMemory::new(50)));
    let tools: Vec<Box<dyn agentverse::SyncTool>> = vec![Box::new(Calculator)];
    let mut agent = ReActStrategy::new(registry, model, tools, memory, 10);

    let question = "What is 42 multiplied by 37, then add 15 to the result?";
    println!("> {}", question);

    match agent.run(question.to_string()).await {
        Ok(result) => {
            println!("\nAgent: {}", result.answer);
            println!(
                "\n[tokens] input={} output={} cache_read={} cache_write={}",
                result.total_usage.input_tokens,
                result.total_usage.output_tokens,
                result.total_usage.cache_read_tokens,
                result.total_usage.cache_write_tokens,
            );
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
