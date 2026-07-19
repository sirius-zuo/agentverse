use std::sync::{Arc, Barrier};
use std::thread;

use agentverse::{
    AgentError as CoreAgentError, ConnectionManager, LlmRunner, PromptRegistry, RunStrategy,
    StrategyOutcome, Tool, ToolResult,
};
use agentverse_agent::Agent;
use agentverse_session::SqliteSessionMemory;
use agentverse_subagent::SubAgentExecutor;
use agentverse_tools::ToolRegistry;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
struct ExistingSpawnSubagentArgs;

struct ExistingSpawnSubagentTool;

#[async_trait]
impl Tool for ExistingSpawnSubagentTool {
    type Args = ExistingSpawnSubagentArgs;

    fn name(&self) -> &str {
        "spawn_subagent"
    }

    fn description(&self) -> &str {
        "existing spawn subagent implementation"
    }

    async fn execute(&self, _args: Self::Args) -> ToolResult {
        Ok(serde_json::json!({ "source": "existing" }))
    }
}

struct NoopStrategy;

#[async_trait]
impl RunStrategy for NoopStrategy {
    async fn run(
        &self,
        _messages: Vec<agentverse::memory::Message>,
    ) -> Result<StrategyOutcome, CoreAgentError> {
        Ok(StrategyOutcome::Done(String::new()))
    }
}

fn connection_manager() -> Arc<ConnectionManager> {
    Arc::new(ConnectionManager::anthropic(
        "http://127.0.0.1:1",
        "claude-sonnet-4-6",
        "test-key",
    ))
}

fn executor(
    connection_manager: Arc<ConnectionManager>,
    tools: Arc<ToolRegistry>,
) -> Arc<SubAgentExecutor> {
    Arc::new(SubAgentExecutor::new(
        connection_manager,
        tools,
        Arc::new(PromptRegistry::new()),
    ))
}

async fn build_agent(
    connection_manager: Arc<ConnectionManager>,
    tools: Arc<ToolRegistry>,
    executor: Arc<SubAgentExecutor>,
) {
    Agent::builder(
        Arc::new(LlmRunner::new(connection_manager)),
        tools,
        Arc::new(PromptRegistry::new()),
        Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap()),
        Arc::new(NoopStrategy),
    )
    .with_subagent_executor(executor)
    .build();
}

#[tokio::test]
async fn agent_builder_with_subagent_executor_registers_spawn_subagent_tool() {
    let tools = ToolRegistry::new();
    assert!(!tools.has_tool("spawn_subagent"));

    let connection_manager = connection_manager();
    let executor = executor(Arc::clone(&connection_manager), Arc::clone(&tools));
    build_agent(connection_manager, Arc::clone(&tools), executor).await;

    assert!(tools.has_tool("spawn_subagent"));
}

#[tokio::test]
async fn agent_builder_after_lower_level_registration_has_one_spawn_subagent_search_result() {
    let tools = ToolRegistry::new();
    let connection_manager = connection_manager();
    let executor = executor(Arc::clone(&connection_manager), Arc::clone(&tools));
    SubAgentExecutor::register_tool(&executor, &tools);

    build_agent(connection_manager, Arc::clone(&tools), executor).await;

    assert_eq!(spawn_subagent_count(&tools), 1);
}

#[tokio::test]
async fn agent_builder_does_not_overwrite_pre_registered_spawn_subagent_tool() {
    let tools = ToolRegistry::new();
    tools.register(ExistingSpawnSubagentTool);
    let connection_manager = connection_manager();
    let executor = executor(Arc::clone(&connection_manager), Arc::clone(&tools));

    build_agent(connection_manager, Arc::clone(&tools), executor).await;

    let results = tools.search("spawn_subagent", 10);
    let spawn_results = results
        .iter()
        .filter(|tool| tool.name == "spawn_subagent")
        .collect::<Vec<_>>();
    assert_eq!(spawn_results.len(), 1);
    assert_eq!(
        spawn_results[0].description,
        "existing spawn subagent implementation"
    );
}

#[tokio::test]
async fn multiple_agent_builders_share_one_spawn_subagent_search_result() {
    let tools = ToolRegistry::new();
    let connection_manager = connection_manager();
    let executor = executor(Arc::clone(&connection_manager), Arc::clone(&tools));

    for _ in 0..2 {
        build_agent(
            Arc::clone(&connection_manager),
            Arc::clone(&tools),
            Arc::clone(&executor),
        )
        .await;
    }

    assert_eq!(spawn_subagent_count(&tools), 1);
}

#[test]
fn concurrent_subagent_registration_if_absent_has_one_active_and_searchable_tool() {
    const WORKERS: usize = 16;
    let tools = ToolRegistry::new();
    let executor = executor(connection_manager(), Arc::clone(&tools));
    let barrier = Arc::new(Barrier::new(WORKERS));
    let handles = (0..WORKERS)
        .map(|_| {
            let tools = Arc::clone(&tools);
            let executor = Arc::clone(&executor);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                SubAgentExecutor::register_tool_if_absent(&executor, &tools)
            })
        })
        .collect::<Vec<_>>();

    let inserted = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|inserted| *inserted)
        .count();
    assert_eq!(inserted, 1);
    assert_eq!(
        tools
            .tool_names()
            .iter()
            .filter(|name| name.as_str() == "spawn_subagent")
            .count(),
        1
    );
    assert_eq!(spawn_subagent_count(&tools), 1);
}

#[test]
fn subagent_executor_register_tool_registers_spawn_subagent_tool() {
    let tools = ToolRegistry::new();
    assert!(!tools.has_tool("spawn_subagent"));

    let executor = executor(connection_manager(), Arc::clone(&tools));
    SubAgentExecutor::register_tool(&executor, &tools);

    assert!(tools.has_tool("spawn_subagent"));
}

fn spawn_subagent_count(tools: &ToolRegistry) -> usize {
    tools
        .search("spawn_subagent", 10)
        .iter()
        .filter(|tool| tool.name == "spawn_subagent")
        .count()
}
