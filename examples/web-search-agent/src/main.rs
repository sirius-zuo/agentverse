// examples/web-search-agent/src/main.rs
//
// Plan-and-Execute agent: search a topic on DuckDuckGo, fetch the top N pages,
// and produce a summarized answer with sources.
//
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-web-search-agent -- "rust async programming" 3

use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::{ToolRegistry, WebSearch};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <topic> <n>", args[0]);
        eprintln!("  topic  Search topic (quote multi-word topics)");
        eprintln!("  n      Number of results to fetch and summarize (1-10)");
        std::process::exit(1);
    }
    let topic = &args[1];
    let n: u64 = match args[2].parse() {
        Ok(v) if v >= 1 => v,
        _ => {
            eprintln!("Error: <n> must be a positive integer (1-10)");
            std::process::exit(1);
        }
    };

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    tracing::info!(model = %model_name, base_url = %base_url, topic = %topic, n = %n, "Web Search Agent");

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
        .expect("runner"),
    );

    let tool_registry = ToolRegistry::new();
    tool_registry.register(WebSearch);
    let tools = tool_registry;

    let prompts = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompts"),
    );

    let strategy = build(
        StrategyKind::Plan,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        5,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("store"),
    );

    let agent = Agent::new(
        runner,
        tools,
        prompts,
        session_memory,
        strategy,
        false,
        None,
        None,
    );

    let question = format!(
        "Search for '{}' and summarize the top {} results.",
        topic, n
    );
    println!("> {}", question);

    match agent.invoke_stateless(&question).await {
        Ok(answer) => println!("\nAgent: {}", answer),
        Err(e) => eprintln!("Error: {}", e),
    }
}
