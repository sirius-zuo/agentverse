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

use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_integration::{Event, IntegrationRuntime};
use agentverse_logging as avs_logging;
use agentverse_memory::SimpleMemory;
use agentverse_session::SqliteSessionStore;
use agentverse_strategy::{build, StrategyKind};
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

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::OpenAI {
                model_name: model_name.clone(),
                api_key,
                base_url: Some(base_url.clone()),
            },
            max_messages: 100,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner"),
    );

    let tools = Arc::new(ToolRegistry::new());

    let prompts = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt config"),
    );

    let memory: Arc<Mutex<dyn agentverse::Memory>> = Arc::new(Mutex::new(SimpleMemory::new(50)));
    let strategy = build(
        StrategyKind::Plan,
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

    let agent = Arc::new(Agent::new(
        runner, tools, prompts, memory, store, strategy, false, None,
    ));

    let config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/agent.toml");
    let runtime = IntegrationRuntime::from_config(config_path)
        .await
        .expect("integration config");

    tracing::info!(model = %model_name, base_url = %base_url, "HR assistant ready");
    runtime
        .run(move |event: Event| {
            let agent = Arc::clone(&agent);
            async move {
                let answer = agent
                    .invoke_stateless(&event.text)
                    .await
                    .map_err(|e| agentverse::AgentError::Memory(e.to_string()))?;
                Ok::<Event, agentverse::AgentError>(Event {
                    text: answer,
                    ..event
                })
            }
        })
        .await
        .expect("integration failed");
}
