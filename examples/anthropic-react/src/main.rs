// examples/anthropic-react/src/main.rs
//
// ReAct agent using AnthropicProvider with prompt caching.
//
// The system prompt lives in prompts/system.j2 — a substantial block that
// intentionally exceeds Anthropic's 1 024-token cache minimum.  It is tagged
// with cache_control: ephemeral on every request.  The first call writes it
// to the cache (cache_write_tokens > 0); subsequent iterations read it back
// at ~10 % of normal input token cost (cache_read_tokens > 0).
// The [tokens] line at the end shows the cumulative split.
//
// Run:
//   ANTHROPIC_API_KEY=sk-ant-... \
//   cargo run -p example-anthropic-react

use agentverse::{AnthropicProvider, PromptConfig, PromptRegistry, ShortTermMemory};
use agentverse_react::ReActStrategy;
use agentverse_tools::Calculator;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("ANTHROPIC_API_KEY is not set");
        std::process::exit(1);
    });
    let model_name = std::env::var("ANTHROPIC_MODEL")
        .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());

    println!("Anthropic ReAct Agent — model: {}", model_name);
    println!("Tool: Calculator");
    println!("Prompt caching: enabled (system prompt + penultimate message)");
    println!();

    let model = Arc::new(AnthropicProvider::new(
        "https://api.anthropic.com",
        &model_name,
        &api_key,
    ));

    // Load the system prompt from prompts/system.j2 (relative to the workspace
    // root, where `cargo run` is invoked).
    let registry = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some("examples/anthropic-react/prompts".to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );

    let memory = Arc::new(Mutex::new(ShortTermMemory::new(50)));
    let tools: Vec<Box<dyn agentverse::SyncTool>> = vec![Box::new(Calculator)];
    let mut agent = ReActStrategy::new(registry, model, tools, memory, 15);

    // Multi-step arithmetic that requires four sequential tool calls:
    //   step 1: 137 * 48  = 6576
    //   step 2: 256 / 4   = 64
    //   step 3: 6576 + 64 = 6640
    //   step 4: 6640 - 19 = 6621
    // Each iteration re-sends the system prompt; after the first write it is
    // served from cache, so cache_read_tokens grows with each loop iteration.
    let question = "What is (137 * 48) + (256 / 4) - 19?";
    println!("> {}", question);

    match agent.run(question.to_string()).await {
        Ok(result) => {
            println!("\nAgent: {}", result.answer);
            println!(
                "\n[tokens] input={} output={} cache_write={} cache_read={}",
                result.total_usage.input_tokens,
                result.total_usage.output_tokens,
                result.total_usage.cache_write_tokens,
                result.total_usage.cache_read_tokens,
            );
            if result.total_usage.cache_read_tokens > 0 {
                println!(
                    "[cache]  {} tokens served from cache across loop iterations",
                    result.total_usage.cache_read_tokens
                );
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
