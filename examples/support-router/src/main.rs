// examples/support-router/src/main.rs
//
// Pattern C — coordinator dispatch.
//
// A coordinator agent (ReAct, no tools) reads the support request and outputs
// a JSON plan: [{skill, task}, ...]. main.rs parses the plan and dispatches
// each step to the appropriate specialist agent, threading the previous step's
// output as context.
//
// Strategies per role:
//   coordinator  → React       (no tools; one-shot JSON output)
//   billing      → Hierarchical (decomposes: lookup invoice → check eligibility → draft)
//   tech-support → React       (single check_service_status call)
//   account-mgmt → React       (single get_account_details call)
//
// Run:
//   ANTHROPIC_API_KEY=sk-ant-... \
//   MODEL_NAME=claude-sonnet-4-6 \
//   cargo run -p example-support-router -- "I was charged twice last month and my API is down"

use agentverse::{Config, LlmRunner, PromptRegistry, ProviderConfig};
use agentverse_agent::{Agent, SkillConfig, SkillMode};
use agentverse_demo_tools::{
    CheckRefundEligibility, CheckServiceStatus, GetAccountDetails, LookupInvoice,
};
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::ToolRegistry;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct PlanStep {
    skill: String,
    task: String,
}

#[tokio::main]
async fn main() {
    avs_logging::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} \"<support request>\"", args[0]);
        std::process::exit(1);
    }
    let request = args[1..].join(" ");

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("MODEL_API_KEY"))
        .unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::anthropic(model_name.clone(), api_key),
            max_messages: 50,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner"),
    );

    let prompts = Arc::new(PromptRegistry::new());
    let skills_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/skills");

    // Coordinator: no tools — the skill instructs it to output only JSON.
    // React with zero tools = one-shot LLM call.
    let coordinator_tools = ToolRegistry::new();
    let coordinator_agent = make_agent(
        &runner,
        &coordinator_tools,
        &prompts,
        StrategyKind::React,
        SkillMode::Open,
        skills_dir,
    )
    .await;

    // Specialists share the same tool registry; skills restrict which tools
    // are active per invocation via their `agentverse.tools` list.
    let specialist_tools = ToolRegistry::new();
    specialist_tools.register(LookupInvoice);
    specialist_tools.register(CheckRefundEligibility);
    specialist_tools.register(CheckServiceStatus);
    specialist_tools.register(GetAccountDetails);

    // billing uses Hierarchical: decomposes into sub-goals, each executed as a plan.
    // This demonstrates that a single dispatch step can itself be a multi-step chain.
    let billing_agent = make_agent(
        &runner,
        &specialist_tools,
        &prompts,
        StrategyKind::Hierarchical,
        SkillMode::Open,
        skills_dir,
    )
    .await;

    let tech_support_agent = make_agent(
        &runner,
        &specialist_tools,
        &prompts,
        StrategyKind::React,
        SkillMode::Open,
        skills_dir,
    )
    .await;

    let account_mgmt_agent = make_agent(
        &runner,
        &specialist_tools,
        &prompts,
        StrategyKind::React,
        SkillMode::Open,
        skills_dir,
    )
    .await;

    // ── 1. Coordinator: produce routing plan ──────────────────────────────
    println!("\n── coordinator ─────────────────────────────────");
    let coord_session = coordinator_agent
        .create_session_with_skill("user", "coordinator")
        .await
        .expect("create coordinator session");

    let plan_json = coordinator_agent
        .invoke("user", coord_session, &request)
        .await
        .unwrap_or_else(|e| {
            eprintln!("coordinator error: {e}");
            std::process::exit(1);
        });

    println!("Plan: {plan_json}");

    let steps = parse_plan(&plan_json.to_string());

    // ── 2. Execute each step with the assigned specialist ─────────────────
    let mut context = String::new();

    for (i, step) in steps.iter().enumerate() {
        println!(
            "\n── step {}: {} ─────────────────────────────────",
            i + 1,
            step.skill
        );

        let specialist = match step.skill.as_str() {
            "billing" => &billing_agent,
            "tech-support" => &tech_support_agent,
            "account-mgmt" => &account_mgmt_agent,
            other => {
                eprintln!("error: unknown skill '{}' in coordinator plan", other);
                std::process::exit(1);
            }
        };

        let input = if context.is_empty() {
            step.task.clone()
        } else {
            format!(
                "Task: {}\n\nContext from previous steps:\n{}",
                step.task, context
            )
        };

        let session_id = specialist
            .create_session_with_skill("user", &step.skill)
            .await
            .unwrap_or_else(|e| {
                eprintln!(
                    "error: create_session_with_skill '{}' failed: {e}",
                    step.skill
                );
                std::process::exit(1);
            });

        let output = specialist
            .invoke("user", session_id, &input)
            .await
            .unwrap_or_else(|e| {
                eprintln!("error: invoke '{}' failed: {e}", step.skill);
                std::process::exit(1);
            });

        println!("{output}");

        if !context.is_empty() {
            context.push('\n');
        }
        context.push_str(&format!("[{}]\n{output}", step.skill));
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
    Agent::builder(
        Arc::clone(runner),
        Arc::clone(tools),
        Arc::clone(prompts),
        session_memory,
        strategy,
    )
    .with_skills(skills)
    .build()
}

/// Parse the coordinator's JSON output into plan steps.
/// Strips markdown fences and finds the first `[...]` array.
fn parse_plan(json: &str) -> Vec<PlanStep> {
    let s = json.trim();
    let s = s.strip_prefix("```json").unwrap_or(s);
    let s = s.strip_prefix("```").unwrap_or(s);
    let s = s.strip_suffix("```").unwrap_or(s);
    let s = s.trim();
    let start = s.find('[').unwrap_or(0);
    let end = s.rfind(']').map(|i| i + 1).unwrap_or(s.len());
    if start > end {
        eprintln!("error: failed to parse coordinator plan: malformed response (']' before '[')\nraw:\n{json}");
        std::process::exit(1);
    }
    let slice = &s[start..end];
    serde_json::from_str(slice).unwrap_or_else(|e| {
        eprintln!("error: failed to parse coordinator plan: {e}\nraw:\n{json}");
        std::process::exit(1);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plan_deserializes_array() {
        let json = r#"[{"skill":"billing","task":"Check invoice"},{"skill":"tech-support","task":"Check status"}]"#;
        let steps = parse_plan(json);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].skill, "billing");
        assert_eq!(steps[1].skill, "tech-support");
    }

    #[test]
    fn parse_plan_strips_markdown_fences() {
        let json = "```json\n[{\"skill\":\"billing\",\"task\":\"Check\"}]\n```";
        let steps = parse_plan(json);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].skill, "billing");
    }

    #[test]
    fn parse_plan_handles_extra_prose_before_array() {
        let json = "Here is your plan:\n[{\"skill\":\"account-mgmt\",\"task\":\"Lookup\"}]";
        let steps = parse_plan(json);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].skill, "account-mgmt");
    }
}
