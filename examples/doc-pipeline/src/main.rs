// examples/doc-pipeline/src/main.rs
//
// Single-agent multi-phase pipeline.
//
// One Agent, one session ID. Three skill phases (extractor → analyzer → summarizer)
// run sequentially within the same session. Each pipeline skill emits:
//   NEXT_SKILL: <id>
//   SUMMARY: <one sentence about what this phase produced>
// advance_phase() parses the directives, rebinds the skill, and stores
// "Summary of previous phase: ..." as the opening context for the next phase.
// The summarizer emits no directives — it is the terminal skill.
//
// Run:
//   ANTHROPIC_API_KEY=sk-ant-... \
//   MODEL_NAME=claude-sonnet-4-6 \
//   cargo run -p example-doc-pipeline -- "your document text here"

use agentverse::{Config, LlmRunner, PromptRegistry, ProviderConfig};
use agentverse_agent::{Agent, SkillConfig, SkillMode};
use agentverse_demo_tools::{CountMentions, FindDates, WordCount};
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::ToolRegistry;
use std::collections::HashSet;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} \"<document text>\"", args[0]);
        std::process::exit(1);
    }
    let input_doc = args[1..].join(" ");

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("MODEL_API_KEY"))
        .unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::Anthropic {
                model_name: model_name.clone(),
                api_key,
            },
            max_messages: 50,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner"),
    );

    let tools = ToolRegistry::new();
    tools.register(FindDates);
    tools.register(CountMentions);
    tools.register(WordCount);

    let prompts = Arc::new(PromptRegistry::new());
    let skills_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/skills");

    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        10,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("session memory"),
    );
    let skills = SkillConfig::load(skills_dir, SkillMode::Open).expect("load skills");

    let agent = Agent::builder(runner, tools, prompts, session_memory, strategy)
        .with_skills(skills)
        .build();

    // One session spans all three phases.
    let session_id = agent
        .create_session_with_skill("user", "extractor")
        .await
        .expect("create session");

    let mut input = input_doc;
    // Track visited skills to detect cycles — a skill emitting NEXT_SKILL pointing back
    // to an already-run skill would loop forever without this guard.
    let mut visited: HashSet<String> = HashSet::from(["extractor".to_string()]);

    loop {
        let output = agent
            .invoke("user", session_id, &input)
            .await
            .unwrap_or_else(|e| {
                eprintln!("error: invoke failed: {e}");
                std::process::exit(1);
            });

        let output_text = output.to_string();
        match agent.advance_phase("user", session_id, &output_text).await {
            Ok(Some(agentverse_agent::PhaseAdvanceResult::Advanced(transition))) => {
                if !visited.insert(transition.next_skill.clone()) {
                    eprintln!(
                        "error: pipeline cycle detected — '{}' has already been visited",
                        transition.next_skill
                    );
                    std::process::exit(1);
                }
                println!("\n── phase complete → {} ──", transition.next_skill);
                input = transition.deliverable;
            }
            Ok(Some(agentverse_agent::PhaseAdvanceResult::Pending { .. })) => {
                eprintln!("error: phase gate pending — manual approval required");
                std::process::exit(1);
            }
            Ok(None) => {
                println!("\n── final output ──────────────────────────");
                println!("{output}");
                break;
            }
            Err(e) => {
                eprintln!("error: advance_phase failed: {e}");
                std::process::exit(1);
            }
        }
    }
}
