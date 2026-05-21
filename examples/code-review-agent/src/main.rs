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

use agentverse::{OpenAICompatible, PromptConfig, PromptRegistry};
use agentverse_memory::SimpleMemory;
use agentverse_plan::HierarchicalStrategy;
use agentverse_tools::{FileSearch, ShellTool, ToolRegistry};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use agentverse_logging as avs_logging;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());
    let project_dir = std::env::var("PROJECT_DIR")
        .unwrap_or_else(|_| "/Users/jinzuo/projects/AgentVerse".to_string());

    tracing::info!(model = %model_name, base_url = %base_url, "Code Review Agent");
    tracing::info!("Strategy: Hierarchical Planning");
    tracing::info!("Tools: FileSearch + ShellTool");

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
    tools.register_with_category(FileSearch, "filesystem");
    // ShellTool lets the agent read file contents with `cat` or search with
    // `grep`. It runs commands in `project_dir` with a 30-second timeout.
    //
    // SECURITY: `workdir` is NOT a filesystem sandbox — absolute paths,
    // symlinks, and `cd` can still reach the full filesystem. The blocked
    // list below prevents the most destructive commands, but for production
    // use consider running the agent inside a container or seccomp sandbox.
    tools.register_with_category(
        ShellTool::new(
            &project_dir,
            Duration::from_secs(30),
            vec![
                "rm".into(),
                "rmdir".into(),
                "mv".into(),
                "dd".into(),
                "sudo".into(),
                "chmod".into(),
                "chown".into(),
            ],
        ),
        "filesystem",
    );

    let mut agent = HierarchicalStrategy::new(
        model, registry, tools, memory, 10, // max_iterations per sub-goal plan
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
