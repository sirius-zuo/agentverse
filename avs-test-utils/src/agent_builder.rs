use agentverse::{Config, LlmRunner, PromptRegistry, ProviderConfig};
use agentverse_agent::{Agent, AgentOutput};
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::ToolRegistry;
use std::sync::Arc;

/// Build an `Arc<Agent>` wired to a dead endpoint — no real LLM calls succeed.
/// Use in tests that only need the agent infrastructure (session, routing, etc.)
/// and handle LLM errors gracefully, or pair with httpmock to stub responses.
pub async fn dead_endpoint_agent() -> Arc<Agent> {
    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::OpenAI {
                model_name: "test-model".into(),
                api_key: "sk-test".into(),
                base_url: Some("http://127.0.0.1:1/v1".into()),
            },
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("LlmRunner::from_config"),
    );
    // ToolRegistry::new() returns Arc<ToolRegistry> — do NOT wrap in Arc::new().
    let tools = ToolRegistry::new();
    let prompts = Arc::new(PromptRegistry::new());
    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        3,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("in-memory sqlite"),
    );
    Agent::builder(runner, tools, prompts, session_memory, strategy).build()
}

/// Resolve `AgentOutput` to its text content, panicking if interrupted.
/// Useful in tests that don't exercise HITL paths.
pub fn unwrap_done(output: AgentOutput) -> String {
    match output {
        AgentOutput::Done(s) => s,
        AgentOutput::Interrupted { approval_id, .. } => {
            panic!("Expected AgentOutput::Done but got Interrupted (id={approval_id})")
        }
    }
}
