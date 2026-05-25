// examples/hello-agent/src/main.rs
//
// Interactive Q&A agent: type a question, get an answer. Type "exit" or Ctrl+C to quit.
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-hello-agent
//
// TODO(Task 4): Restore full interactive loop once ReActStrategy::run() is
// re-implemented against the new CycleSkeleton API.

use agentverse::{Config, LlmRunner, Message, MessageRole, PromptConfig, PromptRegistry};
use agentverse_logging as avs_logging;
use agentverse_react::ReActStrategy;
use agentverse_tools::{Calculator, DateTimeTool, ToolRegistry};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    tracing::info!(model = %model_name, base_url = %base_url, "Hello Agent");

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::OpenAI {
                model_name: model_name.clone(),
                api_key,
                base_url: Some(base_url),
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
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );

    let mut tools = ToolRegistry::new();
    tools.register_with_category(Calculator, "math");
    tools.register_with_category(DateTimeTool, "utility");

    let _agent = ReActStrategy::new(runner, registry, Arc::new(tools), 10);

    // TODO(Task 4): Implement interactive loop using RunStrategy::run(messages)
    // For now, demonstrate that the agent is constructed successfully.
    println!("Hello Agent created (Task 4 will wire the interactive loop).");

    let _example_messages = [Message {
        role: MessageRole::User,
        content: "What time is it?".to_string(),
    }];
}
