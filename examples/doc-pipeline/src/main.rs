// examples/doc-pipeline/src/main.rs
//
// Pattern A — self-directing skill chain.
//
// Three skills form a sequential pipeline. Each non-terminal skill appends
// "NEXT_SKILL: <name>" as its final line, declaring its own successor.
// main.rs runs a loop that strips the directive, passes the clean output
// as input to the next stage, and stops when no directive is emitted.
// No stage names are hardcoded here — the chain lives in the skills.
//
// Strategies per stage:
//   extractor  → React  (calls find_dates to locate timeline events)
//   analyzer   → Plan   (plans which entities to count, calls count_mentions per step)
//   summarizer → React  (calls word_count to enforce a 150-word limit)
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

    // One agent per stage — each uses a different StrategyKind and is
    // constrained to its own skill so it cannot be routed elsewhere.
    let extractor_agent = make_agent(
        &runner,
        &tools,
        &prompts,
        StrategyKind::React,
        SkillMode::Open,
        skills_dir,
    )
    .await;
    let analyzer_agent = make_agent(
        &runner,
        &tools,
        &prompts,
        StrategyKind::Plan,
        SkillMode::Open,
        skills_dir,
    )
    .await;
    let summarizer_agent = make_agent(
        &runner,
        &tools,
        &prompts,
        StrategyKind::React,
        SkillMode::Open,
        skills_dir,
    )
    .await;

    let mut current_skill = "extractor".to_string();
    let mut input = input_doc;
    let mut seen: HashSet<String> = HashSet::new();

    loop {
        if !seen.insert(current_skill.clone()) {
            eprintln!(
                "error: cycle detected — skill '{}' appeared twice",
                current_skill
            );
            std::process::exit(1);
        }

        let stage_agent = match current_skill.as_str() {
            "extractor" => &extractor_agent,
            "analyzer" => &analyzer_agent,
            "summarizer" => &summarizer_agent,
            other => {
                eprintln!("error: unknown skill '{}'", other);
                std::process::exit(1);
            }
        };

        println!(
            "\n── stage: {} ──────────────────────────────",
            current_skill
        );

        let session_id = stage_agent
            .create_session_with_skill("user", &current_skill)
            .await
            .unwrap_or_else(|e| {
                eprintln!("error: create_session_with_skill failed: {e}");
                std::process::exit(1);
            });

        let output = stage_agent
            .invoke("user", session_id, &input)
            .await
            .unwrap_or_else(|e| {
                eprintln!("error: invoke failed: {e}");
                std::process::exit(1);
            });

        let (next_skill, clean_output) = parse_next_skill(&output);

        match next_skill {
            Some(next) => {
                input = clean_output.to_string();
                current_skill = next.to_string();
            }
            None => {
                println!("{}", clean_output);
                break;
            }
        }
    }
}

async fn make_agent(
    runner: &Arc<LlmRunner>,
    tools: &Arc<ToolRegistry>,
    prompts: &Arc<PromptRegistry>,
    strategy_kind: StrategyKind,
    mode: SkillMode,
    skills_dir: &str,
) -> Arc<Agent> {
    let strategy = build(
        strategy_kind,
        Arc::clone(runner),
        Arc::clone(prompts),
        Arc::clone(tools),
        10,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("session memory"),
    );
    let skills = SkillConfig::load(skills_dir, mode).expect("load skills");
    Agent::new(
        Arc::clone(runner),
        Arc::clone(tools),
        Arc::clone(prompts),
        session_memory,
        strategy,
        false,
        None,
        Some(skills),
    )
}

/// Strip `NEXT_SKILL: <name>` from the last non-empty line.
/// Returns `(Some(name), body_without_directive)` or `(None, full_output)`.
fn parse_next_skill(output: &str) -> (Option<&str>, &str) {
    let trimmed = output.trim_end();
    if let Some(last_newline) = trimmed.rfind('\n') {
        let last_line = trimmed[last_newline + 1..].trim();
        if let Some(rest) = last_line.strip_prefix("NEXT_SKILL:") {
            let skill_name = rest.trim();
            let body = trimmed[..last_newline].trim_end();
            return (Some(skill_name), body);
        }
    } else if let Some(rest) = trimmed.strip_prefix("NEXT_SKILL:") {
        return (Some(rest.trim()), "");
    }
    (None, trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strips_next_skill_from_last_line() {
        let out = "Some content.\nMore content.\nNEXT_SKILL: analyzer";
        let (next, body) = parse_next_skill(out);
        assert_eq!(next, Some("analyzer"));
        assert_eq!(body, "Some content.\nMore content.");
    }

    #[test]
    fn parse_returns_none_when_no_directive() {
        let out = "Final summary with no directive.";
        let (next, body) = parse_next_skill(out);
        assert_eq!(next, None);
        assert_eq!(body, "Final summary with no directive.");
    }

    #[test]
    fn parse_handles_trailing_whitespace_after_directive() {
        let out = "Content.\nNEXT_SKILL: summarizer  \n  ";
        let (next, _) = parse_next_skill(out);
        assert_eq!(next, Some("summarizer"));
    }

    #[test]
    fn parse_handles_single_line_output_with_directive() {
        let out = "NEXT_SKILL: analyzer";
        let (next, body) = parse_next_skill(out);
        assert_eq!(next, Some("analyzer"));
        assert_eq!(body, "");
    }
}
