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
//
// TODO(Task 4): Restore full run loop once ReActStrategy::run() is
// re-implemented against the new CycleSkeleton API.

use agentverse::{Config, LlmRunner, Message, MessageRole, PromptConfig, PromptRegistry};
use agentverse_logging as avs_logging;
use agentverse_memory::SimpleMemory;
use agentverse_react::ReActStrategy;
use agentverse_tools::{Calculator, ToolRegistry};
use std::sync::Arc;
use tokio::sync::Mutex;

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
    tracing::info!("Tool: Calculator");
    tracing::info!("Prompt caching: enabled (system prompt + penultimate message)");

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::Anthropic {
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

    let registry = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some("examples/anthropic-react/prompts".to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );

    let mut tools = ToolRegistry::new();
    tools.register(Calculator);

    let _agent = ReActStrategy::new(
        runner,
        registry,
        Arc::new(tools),
        Arc::new(Mutex::new(SimpleMemory::new(0))),
        15,
    );

    // TODO(Task 4): Implement run loop using RunStrategy::run(messages)
    // For now, demonstrate that the agent is constructed successfully.
    let question = "What is (137 * 48) + (256 / 4) - 19?";
    println!("> {}", question);
    println!("Anthropic ReAct agent created (Task 4 will wire the run loop).");

    let _example_messages = [Message {
        role: MessageRole::User,
        content: question.to_string(),
    }];
}
