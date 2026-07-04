// examples/web-search-agent/src/main.rs
//
// Web-search agent constrained to a single skill (SkillMode::Constrained).
//
// Demonstrates two skill-system patterns:
//   Constrained routing: only the "web-search" skill is eligible regardless
//     of what other skills may be present in skills/.
//   Shadow pattern: skills/user/web-search/ has the same `name: web-search`
//     as skills/system/web-search/ — the user variant loads second and
//     silently overrides the system variant, applying stricter citation rules
//     with no code change.
//
// Run:
//   MODEL_BASE_URL=http://localhost:9090/v1 \
//   MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
//   cargo run -p example-web-search-agent -- "rust async programming" 3

use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::{Agent, SkillConfig, SkillMode};
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::{ToolRegistry, WebSearch};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <topic> <n>", args[0]);
        eprintln!("  topic  Search topic (quote multi-word topics)");
        eprintln!("  n      Number of results to fetch and summarize (1-10)");
        std::process::exit(1);
    }
    let topic = &args[1];
    let n: u64 = match args[2].parse() {
        Ok(v) if v >= 1 => v,
        _ => {
            eprintln!("Error: <n> must be a positive integer (1-10)");
            std::process::exit(1);
        }
    };

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    tracing::info!(model = %model_name, base_url = %base_url, topic = %topic, n = %n, "Web Search Agent");

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::openai(model_name.clone(), api_key, Some(base_url)),
            max_messages: 100,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner"),
    );

    let tools = ToolRegistry::new();
    tools.register(WebSearch);

    let prompts = Arc::new(
        PromptRegistry::from_config(&PromptConfig {
            prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
            ..Default::default()
        })
        .expect("prompts"),
    );

    let skills_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/skills");
    // Constrained to "web-search" only. The user/ variant shadows system/ —
    // stricter footnote citation rules activate with no code change.
    let skills = SkillConfig::load(
        skills_dir,
        SkillMode::Constrained(vec!["web-search".to_string()]),
    )
    .expect("failed to load skills — check examples/web-search-agent/skills/");

    let strategy = build(
        StrategyKind::Plan,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        5,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("store"),
    );

    let agent = Agent::builder(runner, tools, prompts, session_memory, strategy)
        .with_skills(skills)
        .build();

    let question = format!(
        "Search for '{}' and summarize the top {} results.",
        topic, n
    );
    println!("> {}", question);

    let session_id = agent.create_session("user").await.expect("session");
    match agent.invoke("user", session_id, &question).await {
        Ok(answer) => println!("\nAgent: {}", answer),
        Err(e) => eprintln!("Error: {}", e),
    }
}
