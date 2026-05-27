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

use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::{FileSearch, ShellTool, ToolOptions, ToolRegistry};
use std::sync::Arc;
use std::time::Duration;

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

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::OpenAI {
                model_name: model_name.clone(),
                api_key,
                base_url: Some(base_url),
            },
            max_messages: 100,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner config"),
    );

    let tools = ToolRegistry::new();
    tools.register_with_options(FileSearch, ToolOptions { category: Some("filesystem".into()), ..Default::default() });
    // ShellTool lets the agent read file contents with `cat` or search with
    // `grep`. It runs commands in `project_dir` with a 30-second timeout.
    //
    // SECURITY: `workdir` is NOT a filesystem sandbox — absolute paths,
    // symlinks, and `cd` can still reach the full filesystem. The blocked
    // list below prevents the most destructive commands, but for production
    // use consider running the agent inside a container or seccomp sandbox.
    tools.register_with_options(
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
        ToolOptions { category: Some("filesystem".into()), ..Default::default() },
    );

    let prompts = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );

    let strategy = build(
        StrategyKind::Hierarchical,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        10,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("session store"),
    );

    let agent = Agent::new(
        runner,
        tools,
        prompts,
        session_memory,
        strategy,
        false,
        None,
    );

    let question = format!(
        "Review the avs-react/src codebase in {}: \
         find all Rust source files, count how many there are, \
         and summarize what each file is responsible for.",
        project_dir
    );
    println!("> {}", question);
    println!();

    match agent.invoke_stateless(&question).await {
        Ok(answer) => println!("Agent:\n{}", answer),
        Err(e) => eprintln!("Error: {}", e),
    }
}
