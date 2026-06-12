use agentverse::{
    Config, ConnectionManager, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig,
};
use agentverse_agent::{Agent, SkillConfig, SkillMode};
use agentverse_demo_tools::{
    MarketSizingCalculator, MilestoneScheduler, RiskAdjustedSchedule, RunwayProjector,
};
use agentverse_logging as avs_logging;
use agentverse_mcp::{McpCatalogSource, McpClient, McpServer, McpTransport};
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_subagent::SubAgentExecutor;
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} \"<company or product description>\"", args[0]);
        std::process::exit(1);
    }
    let subject = args[1..].join(" ");

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    // ── 1. MCP server ──────────────────────────────────────────────────────
    let server_registry = ToolRegistry::new();
    server_registry.register(MarketSizingCalculator);
    server_registry.register(RunwayProjector);
    server_registry.register(MilestoneScheduler);
    server_registry.register(RiskAdjustedSchedule);

    let mut server = McpServer::new(Arc::clone(&server_registry));
    let port = server.bind_random_port().await.expect("bind MCP server");
    tokio::spawn(async move { server.run().await });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // ── 2. MCP client → discover tools ────────────────────────────────────
    let transport = McpTransport::StreamableHttp {
        endpoint: format!("http://127.0.0.1:{port}/mcp")
            .parse()
            .expect("endpoint"),
        headers: Default::default(),
    };
    let client = McpClient::connect(transport).await.expect("connect MCP");
    let mcp_tools = ToolRegistry::new();
    McpCatalogSource::populate(&mcp_tools, &client)
        .await
        .expect("populate MCP tools");

    // ── 3. SubAgentExecutor with MCP tools ─────────────────────────────────
    // Subagents access the domain tools; the main agent gets only spawn_subagent.
    let cm = Arc::new(ConnectionManager::openai(&base_url, &model_name, &api_key));
    let prompts = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompts"),
    );
    let executor = Arc::new(SubAgentExecutor::new(cm, mcp_tools, Arc::clone(&prompts)));

    // ── 4. Agent tool registry: spawn_subagent only ────────────────────────
    let agent_tools = ToolRegistry::new();
    SubAgentExecutor::register_tool(&executor, &agent_tools);

    // ── 5. LlmRunner for the main agent ───────────────────────────────────
    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::OpenAI {
                model_name: model_name.clone(),
                api_key: api_key.clone(),
                base_url: Some(base_url.clone()),
            },
            max_messages: 100,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner"),
    );

    // ── 6. Skill ───────────────────────────────────────────────────────────
    let skills_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/skills");
    let skills = SkillConfig::load(
        skills_dir,
        SkillMode::Constrained(vec!["business-report".to_string()]),
    )
    .expect("load skills");

    // ── 7. Agent ───────────────────────────────────────────────────────────
    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&agent_tools),
        15,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("session memory"),
    );
    let agent = Agent::new(
        runner,
        agent_tools,
        prompts,
        session_memory,
        strategy,
        false,
        None,
        Some(skills),
        None,
    );

    // ── 8. Invoke ──────────────────────────────────────────────────────────
    let question = format!("Generate a business report for: {}", subject);
    println!("> {}\n", question);

    let session_id = agent.create_session("user").await.expect("create session");

    match agent.invoke("user", session_id, &question).await {
        Ok(answer) => println!("{}", answer),
        Err(e) => eprintln!("Error: {}", e),
    }
}
