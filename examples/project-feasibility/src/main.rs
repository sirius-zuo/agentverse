use agentverse::{ConnectionManager, PromptConfig, PromptRegistry};
use agentverse_demo_tools::{
    MilestoneScheduler, NpvCalculator, ProjectCostEstimator, RiskAdjustedSchedule,
};
use agentverse_logging as avs_logging;
use agentverse_mcp::{McpCatalogSource, McpClient, McpServer, McpTransport};
use agentverse_subagent::{Budget, ResourceContent, SubAgentContext, SubAgentExecutor, SubAgentSpec};
use agentverse_tools::ToolRegistry;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} \"<project description>\"", args[0]);
        std::process::exit(1);
    }
    let project = args[1..].join(" ");

    let base_url = std::env::var("MODEL_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name = std::env::var("MODEL_NAME")
        .unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    // ── 1. MCP server ──────────────────────────────────────────────────────
    let server_registry = ToolRegistry::new();
    server_registry.register(ProjectCostEstimator);
    server_registry.register(NpvCalculator);
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
    let discovered = McpCatalogSource::populate(&mcp_tools, &client)
        .await
        .expect("populate MCP tools");
    tracing::info!(discovered, "MCP tools discovered");

    // ── 3. Executor ────────────────────────────────────────────────────────
    let cm = Arc::new(ConnectionManager::openai(&base_url, &model_name, &api_key));
    let prompts = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(
                concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string(),
            ),
            ..Default::default()
        })
        .expect("prompts"),
    );
    let executor = SubAgentExecutor::new(cm, mcp_tools, prompts);

    // ── 4. Stage 1: three analysts in parallel ─────────────────────────────
    let base_ctx = SubAgentContext { resources: vec![], depth: 0 };

    let tasks = vec![
        (
            SubAgentSpec {
                name: "financial-analyst".into(),
                system_prompt: Some(
                    "You are a financial analyst. Use project_cost_estimator to estimate \
                     total development cost and npv_calculator to evaluate long-term return. \
                     Be specific with numbers."
                        .into(),
                ),
                objective: format!(
                    "Estimate total development cost, NPV, and 3-year ROI for: {}. \
                     Assume 12% discount rate and 18-month development duration.",
                    project
                ),
                model: None,
                allowed_tools: vec![
                    "project_cost_estimator".into(),
                    "npv_calculator".into(),
                ],
                budget: Budget {
                    max_steps: 8,
                    max_tokens: 4000,
                    timeout: Duration::from_secs(90),
                },
            },
            base_ctx.clone(),
        ),
        (
            SubAgentSpec {
                name: "timeline-analyst".into(),
                system_prompt: Some(
                    "You are a project timeline analyst. Use milestone_scheduler to \
                     project delivery phases from today's date."
                        .into(),
                ),
                objective: format!(
                    "Project a realistic delivery timeline for: {}. \
                     Start from today. Include phases: Discovery (4w), MVP (12w), \
                     Beta (8w), GA (4w). Each phase depends on the previous.",
                    project
                ),
                model: None,
                allowed_tools: vec!["milestone_scheduler".into()],
                budget: Budget {
                    max_steps: 5,
                    max_tokens: 3000,
                    timeout: Duration::from_secs(60),
                },
            },
            base_ctx.clone(),
        ),
        (
            SubAgentSpec {
                name: "risk-analyst".into(),
                system_prompt: Some(
                    "You are a risk analyst. Use risk_adjusted_schedule to quantify \
                     schedule uncertainty. Identify the top 5 technical and business risks."
                        .into(),
                ),
                objective: format!(
                    "Identify top 5 risks and quantify schedule risk using PERT \
                     estimates for: {}",
                    project
                ),
                model: None,
                allowed_tools: vec!["risk_adjusted_schedule".into()],
                budget: Budget {
                    max_steps: 6,
                    max_tokens: 3000,
                    timeout: Duration::from_secs(60),
                },
            },
            base_ctx.clone(),
        ),
    ];

    println!("\nRunning 3 analyst subagents in parallel...\n");
    let results = executor.run_many(tasks).await;

    // ── 5. Collect results as ResourceContent ──────────────────────────────
    let labels = ["Financial Analysis", "Timeline Analysis", "Risk Analysis"];
    let mut resources = Vec::new();
    for (label, result) in labels.iter().zip(results.iter()) {
        match result {
            Ok(r) => {
                println!("  [ok] {} ({} steps)", label, r.steps);
                resources.push(ResourceContent {
                    label: label.to_string(),
                    content: r.answer.clone(),
                });
            }
            Err(e) => {
                println!("  [err] {}: {}", label, e);
                resources.push(ResourceContent {
                    label: label.to_string(),
                    content: "[analysis unavailable]".into(),
                });
            }
        }
    }

    // ── 6. Stage 2: synthesis ──────────────────────────────────────────────
    println!("\nSynthesizing feasibility report...\n");
    let synthesis_spec = SubAgentSpec {
        name: "synthesis".into(),
        system_prompt: Some(
            "You are a senior consultant. Synthesize multi-domain analyses into a \
             clear executive report with an actionable recommendation."
                .into(),
        ),
        objective: "Based on the financial, timeline, and risk analyses in the context, \
                    write a structured Project Feasibility Report with sections: \
                    Executive Summary, Financial Outlook, Delivery Timeline, Risk Profile, \
                    and a final verdict — PROCEED / HOLD / REJECT — with justification."
            .into(),
        model: None,
        allowed_tools: vec![],
        budget: Budget {
            max_steps: 5,
            max_tokens: 6000,
            timeout: Duration::from_secs(90),
        },
    };
    let synthesis_ctx = SubAgentContext { resources, depth: 0 };

    match executor.run(&synthesis_spec, synthesis_ctx).await {
        Ok(r) => println!("{}", r.answer),
        Err(e) => eprintln!("Synthesis failed: {}", e),
    }
}
