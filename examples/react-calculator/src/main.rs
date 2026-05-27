// examples/react-calculator/src/main.rs
//
// Demonstrates the multi-step ReAct loop with the Calculator tool.
// Type an arithmetic question; the agent breaks it into sequential tool calls,
// reasoning before each one, and produces a final answer when done.
//
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-react-calculator

use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::{Calculator, ToolOptions, ToolRegistry};
use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main]
async fn main() {
    avs_logging::init();

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    tracing::info!(model = %model_name, base_url = %base_url, "ReAct Calculator");
    println!("Type an arithmetic question. Type \"exit\" or press Ctrl+C to quit.\n");

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
    tools.register_with_options(
        Calculator,
        ToolOptions {
            category: Some("math".into()),
            ..Default::default()
        },
    );

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
