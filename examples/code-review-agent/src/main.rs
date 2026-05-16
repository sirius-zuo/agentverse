// examples/code-review-agent/src/main.rs
//
// Demonstrates the Hierarchical Planning strategy:
//   1. Decompose — model breaks the request into independent sub-goals
//   2. Plan      — for each sub-goal a step-by-step plan is generated
//   3. Execute   — each plan step runs a tool or reasons inline
//   4. Synthesize — all results are combined into a final answer
//
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   PROJECT_DIR=/path/to/AgentVerse \
//   cargo run -p example-code-review-agent

use agentverse::{OpenAICompatible, PromptConfig, PromptRegistry, ShortTermMemory};
use agentverse_plan::HierarchicalStrategy;
use agentverse_tools::{Calculator, FileSearch};
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());
    let project_dir = std::env::var("PROJECT_DIR")
        .unwrap_or_else(|_| "/Users/jinzuo/projects/AgentVerse".to_string());

    println!("Code Review Agent — model: {} @ {}", model_name, base_url);
    println!("Strategy: Hierarchical Planning");
    println!("Tools: FileSearch + Calculator");
    println!();

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
    let tools: Vec<Box<dyn agentverse::SyncTool>> =
        vec![Box::new(FileSearch), Box::new(Calculator)];

    let mut agent = HierarchicalStrategy::new(
        model,
        registry,
        tools,
        memory,
        10, // max_iterations per sub-goal plan
        5,  // max_decompose_depth
    );

    let question = format!(
        "Review the avs-react/src codebase in {}: \
         find all Rust source files, count how many there are, \
         and summarize what each file is responsible for.",
        project_dir
    );
    println!("> {}", question);
    println!();

    match agent.run(question).await {
        Ok(answer) => println!("Agent:\n{}", answer),
        Err(e) => eprintln!("Error: {}", e),
    }
}
