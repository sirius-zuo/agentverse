// examples/mcp-demo/src/main.rs
//
// Demonstrates the full MCP round-trip in a single process:
//
//   Server side  — a ToolRegistry with Calculator + DateTime is exposed
//                  as an MCP endpoint via McpServer.
//
//   Client side  — McpCatalogSource discovers those tools through the MCP
//                  protocol and registers them into a fresh ToolRegistry.
//                  An agent then uses those tools without knowing they are
//                  remote.
//
// This pattern is useful when tools live in a separate service (different
// language, different host, or a third-party MCP server such as those
// published on npm / PyPI).
//
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-mcp-demo

use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_logging as avs_logging;
use agentverse_mcp::{McpCatalogSource, McpClient, McpServer, McpTransport};
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::{Calculator, DateTimeTool, ToolRegistry};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    avs_logging::init();

    // ── 1. Build the server-side registry ────────────────────────────────────
    //
    // In a real deployment this registry lives in a separate process or host.
    // Here we run it in the same process for simplicity.
    let server_registry = ToolRegistry::new();
    server_registry.register(Calculator);
    server_registry.register(DateTimeTool);

    tracing::info!(tools = server_registry.len(), "Server registry ready");

    // ── 2. Start the MCP server ───────────────────────────────────────────────
    let mut server = McpServer::new(Arc::clone(&server_registry));
    let port = server
        .bind_random_port()
        .await
        .expect("bind MCP server port");

    tokio::spawn(async move { server.run().await });

    // Give the server a moment to start accepting connections.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    tracing::info!(port, "MCP server listening");

    // ── 3. Connect the client and discover tools ──────────────────────────────
    //
    // McpCatalogSource calls tools/list and registers each result as an
    // McpToolAdapter (implements ErasedTool, forwards execute_raw to the server).
    let transport = McpTransport::StreamableHttp {
        endpoint: format!("http://127.0.0.1:{port}/mcp")
            .parse()
            .expect("parse endpoint URL"),
        headers: Default::default(),
    };
    let client = McpClient::connect(transport)
        .await
        .expect("connect to MCP server");

    let client_registry = ToolRegistry::new();
    let discovered = McpCatalogSource::populate(&client_registry, &client)
        .await
        .expect("populate registry from MCP server");

    tracing::info!(discovered, "Tools registered from MCP server");

    println!("Discovered {} tool(s) via MCP:", discovered);
    for name in client_registry.tool_names() {
        // skip the auto-registered find_tools meta-tool
        if name != "find_tools" {
            println!("  - {name}");
        }
    }
    println!();

    // ── 4. Build an agent that uses the MCP-sourced tools ─────────────────────
    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    tracing::info!(model = %model_name, "Starting agent");

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::OpenAI {
                model_name,
                api_key,
                base_url: Some(base_url),
            },
            max_messages: 100,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner config"),
    );

    let prompts = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompt registry"),
    );

    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&client_registry),
        10,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("session store"),
    );

    let agent = Agent::builder(runner, client_registry, prompts, session_memory, strategy).build();

    // ── 5. Run a query ────────────────────────────────────────────────────────
    //
    // The agent uses the calculator and datetime tools it discovered over MCP.
    // It has no idea those tools are backed by a remote server.
    let question = "What is 123 multiplied by 456, and what is today's date?";
    println!("Question: {question}\n");

    match agent.invoke_stateless(question).await {
        Ok(answer) => println!("Answer:\n{answer}"),
        Err(e) => eprintln!("Error: {e}"),
    }
}
