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
//   cargo run -p example-accountant-workflow

use agentverse::{Config, LlmRunner, PromptRegistry, ProviderConfig};
use agentverse_agent::agent::HitlConfig;
use agentverse_agent::{
    parse_phase_transition, Agent, AgentOutput, PhaseAdvanceResult, SkillConfig, SkillMode,
};
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

    let base_url =
        std::env::var("MODEL_BASE_URL").unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name =
        std::env::var("MODEL_NAME").unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::openai(model_name, api_key, Some(base_url)),
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

    // Derive the HITL policy directly from what the SKILL.md files declare.
    let policy = {
        let reg = skills.registry.read().await;
        let mut policy = HitlPolicy::new();
        for skill in reg.eligible(&SkillMode::Open) {
            if skill.phase_gate {
                policy.skill_phase_gates.insert(skill.id.clone());
            }
            if !skill.hitl_tools.is_empty() {
                policy
                    .skill_tool_gates
                    .insert(skill.id.clone(), skill.hitl_tools.iter().cloned().collect());
            }
            if !skill.checkpoints.is_empty() {
                policy
                    .skill_checkpoints
                    .insert(skill.id.clone(), skill.checkpoints.clone());
            }
        }
        policy
    };

    let queue = Arc::new(InMemoryQueue::new());
    let hitl = HitlConfig {
        policy,
        queue: Arc::clone(&queue) as Arc<dyn agentverse_hitl::ApprovalQueue>,
    };

    let agent = Agent::builder(
        Arc::clone(&runner),
        Arc::clone(&tools),
        Arc::clone(&prompts),
        session_memory,
        strategy,
    )
    .with_skills(skills)
    .with_hitl(hitl)
    .build();

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
                        // Phase gate: show the deliverable for human review.
                        // Only save the deliverable when approved — a rejection lets the
                        // advance_phase None branch below handle termination cleanly.
                        let deliverable = parse_phase_transition(&text)
                            .map(|t| t.deliverable)
                            .unwrap_or_else(|| text.clone());
                        println!("\n══ HITL — Phase Gate ═════════════════════════");
                        println!("{deliverable}");
                        let decision = read_decision("Approve transition to next phase?");
                        if matches!(
                            decision,
                            ApprovalDecision::Approved | ApprovalDecision::Modified { .. }
                        ) {
                            pending_deliverable = Some(deliverable);
                        }
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
        InterruptKind::PhaseGate { .. } => {
            // Phase gates are submitted inside advance_phase and surface as
            // PhaseAdvanceResult::Pending — never as AgentOutput::Interrupted.
            unreachable!("PhaseGate interrupt cannot arrive via AgentOutput::Interrupted")
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
