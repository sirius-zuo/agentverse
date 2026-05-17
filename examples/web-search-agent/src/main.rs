// examples/web-search-agent/src/main.rs
//
// Agent with FileSearch tool — exercises multi-step tool use.
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-web-search-agent

use agentverse::{OpenAICompatible, PromptConfig, PromptRegistry};
use agentverse_memory::SimpleMemory;
use agentverse_react::ReActStrategy;
use agentverse_tools::FileSearch;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());
    let project_dir = std::env::var("PROJECT_DIR")
        .unwrap_or_else(|_| "/Users/jinzuo/projects/AgentVerse".to_string());

    println!("Web Search Agent — model: {} @ {}", model_name, base_url);
    println!("Tool: FileSearch (project: {})", project_dir);

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
    let memory = Arc::new(Mutex::new(SimpleMemory::new(50)));
    let tools: Vec<Box<dyn agentverse::SyncTool>> = vec![Box::new(FileSearch)];
    let mut agent = ReActStrategy::new(registry, model, tools, memory, 10);

    let question = format!(
        "Use the file_search tool to find all .rs files in {}/avs-core/src and list their names.",
        project_dir
    );
    println!("> {}", question);

    match agent.run(question).await {
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
