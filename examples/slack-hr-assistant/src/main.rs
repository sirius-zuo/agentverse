// examples/slack-hr-assistant/src/main.rs
//
// Slack HR assistant — agent owns its integration.
// Reads integration config from agent.toml; falls back to console if not found.
//
// Run:
//   SLACK_BOT_TOKEN=xoxb-... \
//   SLACK_SIGNING_SECRET=... \
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=your-model \
//   cargo run -p example-slack-hr-assistant
use agentverse::{OpenAICompatible, PromptConfig, PromptRegistry, RunStrategy};
use agentverse_integration::{Event, IntegrationRuntime};
use agentverse_memory::SimpleMemory;
use agentverse_plan::PlanStrategy;
use agentverse_logging as avs_logging;
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".into());
    let model_name = std::env::var("MODEL_NAME").unwrap_or_else(|_| "gpt-4".into());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();

    let model = Arc::new(OpenAICompatible::new(&base_url, &model_name, &api_key));
    let registry = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );
    let memory = Arc::new(Mutex::new(SimpleMemory::new(50)));
    let tools = ToolRegistry::new();
    let strategy = Arc::new(Mutex::new(PlanStrategy::new(
        model, registry, tools, memory, 10,
    )));

    let config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/agent.toml");
    let runtime = IntegrationRuntime::from_config(config_path)
        .await
        .expect("integration config");

    tracing::info!(model = %model_name, base_url = %base_url, "HR assistant ready");
    runtime
        .run(move |event: Event| {
            let strategy = Arc::clone(&strategy);
            async move {
                let answer = strategy.lock().await.process(event.text).await?;
                Ok::<Event, agentverse::AgentError>(Event {
                    text: answer,
                    ..event
                })
            }
        })
        .await
        .expect("integration failed");
}
