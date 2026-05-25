// examples/hello-agent/src/main.rs
//
// Interactive Q&A agent: type a question, get an answer. Type "exit" or Ctrl+C to quit.
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-hello-agent

use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_logging as avs_logging;
use agentverse_memory::SimpleMemory;
use agentverse_session::SqliteSessionStore;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::{Calculator, DateTimeTool, ToolRegistry};
use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    tracing::info!(model = %model_name, base_url = %base_url, "Hello Agent");
    println!("Type your question and press Enter. Type \"exit\" or press Ctrl+C to quit.\n");

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
        .expect("failed to build runner"),
    );

    let mut tool_registry = ToolRegistry::new();
    tool_registry.register_with_category(Calculator, "math");
    tool_registry.register_with_category(DateTimeTool, "utility");
    let tools = Arc::new(tool_registry);

    let prompts = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );

    let memory: Arc<Mutex<dyn agentverse::Memory>> = Arc::new(Mutex::new(SimpleMemory::new(50)));
    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        10,
    );
    let store = Arc::new(
        SqliteSessionStore::new("sqlite::memory:")
            .await
            .expect("session store"),
    );

    let agent = Agent::new(runner, tools, prompts, memory, store, strategy, false);

    let session_id = agent.create_session("user").await.expect("create session");

    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    loop {
        print!("You: ");
        std::io::stdout().flush().ok();

        let input = match lines.next_line().await {
            Ok(Some(line)) => line,
            _ => {
                println!("\nGoodbye!");
                break;
            }
        };

        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }
        if input.eq_ignore_ascii_case("exit") {
            println!("Goodbye!");
            break;
        }

        match agent.invoke("user", session_id, &input).await {
            Ok(answer) => println!("\nAgent: {}\n", answer),
            Err(e) => eprintln!("Error: {}\n", e),
        }
    }
}
