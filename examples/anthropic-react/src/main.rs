// examples/anthropic-react/src/main.rs
//
// ReAct agent using Anthropic provider with prompt caching.
//
// The system prompt lives in prompts/system.j2 — a substantial block that
// intentionally exceeds Anthropic's 1024-token cache minimum.  It is tagged
// with cache_control: ephemeral on every request.  The first call writes it
// to the cache (cache_write_tokens > 0); subsequent iterations read it back
// at ~10% of normal input token cost (cache_read_tokens > 0).
//
// Run:
//   ANTHROPIC_API_KEY=sk-ant-... \
//   cargo run -p example-anthropic-react

use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::{Calculator, ToolRegistry};
use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main]
async fn main() {
    avs_logging::init();

    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        tracing::error!("ANTHROPIC_API_KEY is not set");
        std::process::exit(1);
    });
    let model_name = std::env::var("ANTHROPIC_MODEL")
        .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());

    tracing::info!(model = %model_name, "Anthropic ReAct Agent");
    println!("Type an arithmetic question. Type \"exit\" or press Ctrl+C to quit.\n");

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::Anthropic {
                model_name: model_name.clone(),
                api_key,
            },
            max_messages: 50,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner config"),
    );

    let tool_registry = ToolRegistry::new();
    tool_registry.register(Calculator);
    let tools = tool_registry;

    let prompts = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );

    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        15,
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
        None,
    );

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

        match agent.invoke_stateless(&input).await {
            Ok(answer) => println!("\nAgent: {}\n", answer),
            Err(e) => eprintln!("Error: {}\n", e),
        }
    }
}
