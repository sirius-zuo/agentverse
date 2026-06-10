// examples/http-agent/src/main.rs
//
// HTTP-serving agent. Builds an Agent with enable_http_server=true.
// The Agent spawns the HTTP server internally as a background tokio task.
// This binary just keeps the process alive until Ctrl-C.
//
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-http-agent

use agentverse::{Config, LlmRunner, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
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

    tracing::info!(model = %model_name, "Building HTTP agent");

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
    tool_registry.register(Calculator);
    tool_registry.register(DateTimeTool);
    let tools = tool_registry;

    let prompts = Arc::new(PromptRegistry::new());
    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        10,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite:agent.db")
            .await
            .expect("session store"),
    );

    // enable_http_server=true: Agent reads HOST/PORT from env and spawns HTTP server internally
    let _agent = Agent::new(
        runner,
        tools,
        prompts,
        session_memory,
        strategy,
        true,
        None,
        None,
    );

    tracing::info!("Agent started. Press Ctrl-C to stop.");
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl-C handler");
    tracing::info!("Shutting down.");
}
