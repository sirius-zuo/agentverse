// examples/accountant-workflow/src/main.rs
//
// Single-agent multi-phase accounting pipeline with HITL gates.
//
// Three skill phases:
//   extract-transactions  — categorises CSV rows. No HITL.
//   prepare-journal-entry — drafts a journal entry.
//     checkpoint "draft_ready": human reviews draft before execution continues.
//     phase_gate: human approves the completed entry before the next phase starts.
//   submit-to-ledger      — posts the approved entry to the ledger.
//     hitl_tools ["ledger_post"]: human approves the ledger write.
//
// Approval backend: InMemoryQueue with console stdin prompts.
// To use a production backend, replace `Arc::new(InMemoryQueue::new())` with
// any type implementing `agentverse_hitl::ApprovalQueue` (e.g. a Slack queue).
//
// Run:
//   ANTHROPIC_API_KEY=sk-ant-... \
//   MODEL_NAME=claude-sonnet-4-6 \
//   cargo run -p example-accountant-workflow

use agentverse::{Config, LlmRunner, PromptRegistry, ProviderConfig};
use agentverse_agent::agent::HitlConfig;
use agentverse_agent::{Agent, AgentOutput, PhaseAdvanceResult, SkillConfig, SkillMode};
use agentverse_demo_tools::LedgerPost;
use agentverse_hitl::{
    ApprovalDecision, HitlPolicy, InMemoryQueue, InterruptKind, RequestCheckpointTool,
};
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::ToolRegistry;
use std::io::Write;
use std::sync::Arc;
use uuid::Uuid;

/// Sample CSV passed as Phase 1 input.
const SAMPLE_CSV: &str = "\
Date,Description,Amount
2026-06-01,Office rent,-2000.00
2026-06-03,Client payment received,5000.00
2026-06-05,Software subscription,-99.00
2026-06-10,Contractor invoice,-1200.00";

#[tokio::main]
async fn main() {
    avs_logging::init();

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("MODEL_API_KEY"))
        .unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::Anthropic {
                model_name,
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
    tools.register(RequestCheckpointTool);
    tools.register(LedgerPost);

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

    // Build the HITL policy from system-skill declarations (mirrors what the SKILL.md files declare).
    let mut policy = HitlPolicy::new();
    policy
        .skill_phase_gates
        .insert("prepare-journal-entry".to_string());
    policy.skill_checkpoints.insert(
        "prepare-journal-entry".to_string(),
        vec!["draft_ready".to_string()],
    );
    policy.skill_tool_gates.insert(
        "submit-to-ledger".to_string(),
        ["ledger_post"].iter().map(|s| s.to_string()).collect(),
    );

    let queue = Arc::new(InMemoryQueue::new());
    let hitl = HitlConfig {
        policy,
        queue: Arc::clone(&queue) as Arc<dyn agentverse_hitl::ApprovalQueue>,
    };

    let agent = Agent::new(
        Arc::clone(&runner),
        Arc::clone(&tools),
        Arc::clone(&prompts),
        session_memory,
        strategy,
        false,
        None,
        Some(skills),
        Some(hitl),
    );

    let session_id = agent
        .create_session_with_skill("user", "extract-transactions")
        .await
        .expect("create session");

    println!("\n── accountant workflow ───────────────────────────");
    println!("Input CSV:\n{SAMPLE_CSV}\n");

    run_loop(&agent, session_id, SAMPLE_CSV.to_string()).await;
}

/// Drives the session through all skill phases, pausing at HITL interrupts.
///
/// The loop has two modes each iteration:
/// - Normal invoke: call `agent.invoke` with the current input.
/// - Resume: call `agent.resume` with a stored (approval_id, decision) pair.
///
/// After every `Done` output:
/// - If a deliverable was saved for a pending phase gate, use it as the next input.
/// - Otherwise call `advance_phase` to detect phase transitions or terminal output.
async fn run_loop(agent: &Arc<Agent>, session_id: Uuid, initial_input: String) {
    let mut input = initial_input;
    // Set when advance_phase returns Pending (phase gate). Cleared when used as next input.
    let mut pending_deliverable: Option<String> = None;
    // Set when an interrupt needs resolving. Cleared at the start of each iteration.
    let mut resume_from: Option<(Uuid, ApprovalDecision)> = None;

    loop {
        let result = if let Some((aid, decision)) = resume_from.take() {
            agent.resume("user", session_id, aid, decision).await
        } else {
            agent.invoke("user", session_id, &input).await
        };

        match result.unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        }) {
            AgentOutput::Interrupted { approval_id, kind } => {
                let decision = prompt_approval(&kind);
                resume_from = Some((approval_id, decision));
            }

            AgentOutput::Done(text) => {
                // After a phase gate resume, the phase context is already applied.
                // Use the stored deliverable as input for the next invoke.
                if let Some(deliverable) = pending_deliverable.take() {
                    println!("\n── phase approved — starting next phase ─────────");
                    input = deliverable;
                    continue;
                }

                match agent
                    .advance_phase("user", session_id, &text)
                    .await
                    .unwrap_or_else(|e| {
                        eprintln!("error: advance_phase: {e}");
                        std::process::exit(1);
                    }) {
                    Some(PhaseAdvanceResult::Advanced(transition)) => {
                        println!(
                            "\n── → {} ──────────────────────────────────────",
                            transition.next_skill
                        );
                        input = transition.deliverable;
                    }

                    Some(PhaseAdvanceResult::Pending { approval_id }) => {
                        // Phase gate: show the deliverable for human review, then store it
                        // for the next invoke after the resume returns Done.
                        let deliverable = extract_deliverable(&text);
                        println!("\n══ HITL — Phase Gate ═════════════════════════");
                        println!("{deliverable}");
                        let decision = read_decision("Approve transition to next phase?");
                        pending_deliverable = Some(deliverable);
                        resume_from = Some((approval_id, decision));
                    }

                    None => {
                        println!("\n── final output ──────────────────────────────");
                        println!("{text}");
                        return;
                    }
                }
            }
        }
    }
}

/// Displays interrupt details and prompts for an approve/reject decision via stdin.
fn prompt_approval(kind: &InterruptKind) -> ApprovalDecision {
    match kind {
        InterruptKind::SkillCheckpoint {
            checkpoint_name,
            payload,
        } => {
            println!(
                "\n══ HITL — Checkpoint: {} ══════════════════════",
                checkpoint_name
            );
            println!(
                "{}",
                serde_json::to_string_pretty(payload).unwrap_or_default()
            );
            read_decision("Approve checkpoint?")
        }
        InterruptKind::PhaseGate {
            from_skill,
            to_skill,
            deliverable,
        } => {
            println!(
                "\n══ HITL — Phase Gate: {} → {} ═════════════════",
                from_skill, to_skill
            );
            println!("{deliverable}");
            read_decision("Approve phase transition?")
        }
        InterruptKind::ToolApproval { tool_name, args } => {
            println!(
                "\n══ HITL — Tool Approval: {} ════════════════════",
                tool_name
            );
            println!("{}", serde_json::to_string_pretty(args).unwrap_or_default());
            read_decision(&format!("Approve call to {tool_name}?"))
        }
    }
}

/// Prints a prompt and reads a y/N decision from stdin.
fn read_decision(prompt: &str) -> ApprovalDecision {
    print!("{prompt} [y/N]: ");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap_or(0);
    if line.trim().eq_ignore_ascii_case("y") {
        ApprovalDecision::Approved
    } else {
        ApprovalDecision::Rejected {
            reason: "rejected by operator".to_string(),
        }
    }
}

/// Strips NEXT_SKILL and SUMMARY directives from LLM output, returning the deliverable body.
/// Mirrors the internal logic of `parse_phase_transition` in avs-agent.
pub fn extract_deliverable(output: &str) -> String {
    let mut lines: Vec<&str> = output.trim_end().lines().collect();
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    if lines
        .last()
        .map(|l| l.trim().starts_with("SUMMARY:"))
        .unwrap_or(false)
    {
        lines.pop();
    }
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    if lines
        .last()
        .map(|l| l.trim().starts_with("NEXT_SKILL:"))
        .unwrap_or(false)
    {
        lines.pop();
    }
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_both_directives() {
        let output = "Transactions:\n- Rent: -$2000\n- Payment: +$5000\n\nNEXT_SKILL: prepare-journal-entry\nSUMMARY: Found 2 transactions";
        assert_eq!(
            extract_deliverable(output),
            "Transactions:\n- Rent: -$2000\n- Payment: +$5000"
        );
    }

    #[test]
    fn no_directives_returns_unchanged() {
        let output = "Ledger posted. Confirmation: CONF-JE-2026-06.";
        assert_eq!(extract_deliverable(output), output);
    }

    #[test]
    fn handles_trailing_blank_lines_around_directives() {
        let output = "Body text\n\nNEXT_SKILL: foo\nSUMMARY: bar\n\n";
        assert_eq!(extract_deliverable(output), "Body text");
    }
}
