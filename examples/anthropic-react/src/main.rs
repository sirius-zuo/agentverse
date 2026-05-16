// examples/anthropic-react/src/main.rs
//
// ReAct agent using AnthropicProvider with prompt caching.
//
// The system prompt (with tool descriptions) is tagged with
// cache_control: ephemeral on every call.  After the first call writes it to
// Anthropic's cache, subsequent iterations within the same run read it back at
// ~10 % of the normal input token cost.  The [tokens] line shows the split.
//
// Run:
//   ANTHROPIC_API_KEY=sk-ant-... \
//   cargo run -p example-anthropic-react

use agentverse::{AnthropicProvider, PromptRegistry, ShortTermMemory};
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
    let registry = Arc::new(PromptRegistry::default());
    let memory = Arc::new(Mutex::new(ShortTermMemory::new(50)));
    let tools: Vec<Box<dyn agentverse::SyncTool>> = vec![Box::new(Calculator)];
    let mut agent = ReActStrategy::new(registry, model, tools, memory, 10);

    // Multi-step arithmetic — exercises the tool loop across several iterations.
    // Each iteration re-sends the system prompt; after the first, the cache
    // serves it, so cache_read_tokens grows while cache_write_tokens drops to 0.
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
