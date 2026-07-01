# accountant-workflow

A three-phase accounting pipeline that demonstrates all three HITL gate types on top of a self-directing skill chain.

## What this shows

**Pattern A (self-directing chain) + HITL** — Each phase declares its own successor via `NEXT_SKILL: <name>` (same mechanism as `doc-pipeline`), but here every phase also gates part of its work behind human approval:

| Phase | Skill | HITL gate | Approval fires on |
|---|---|---|---|
| 1 | `extract-transactions` | none | — |
| 2 | `prepare-journal-entry` | skill checkpoint (`draft_ready`) + phase gate | `request_checkpoint` call, then `advance_phase` into `submit-to-ledger` |
| 3 | `submit-to-ledger` | tool approval | the `ledger_post` tool call |

The gates are declared entirely in `SKILL.md` frontmatter (`checkpoints`, `phase_gate`, `hitl_tools`) — `main.rs` derives the `HitlPolicy` from whatever the loaded skills declare, so adding or removing a gate never touches Rust code.

**Approval backend** — `InMemoryQueue` with console `stdin` prompts. Swap in any type implementing `agentverse_hitl::ApprovalQueue` (e.g. `SqliteQueue`, or a Slack-backed queue) to move this into production.

## How to run

| Variable | Required | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | Yes | Anthropic API key (`MODEL_API_KEY` also accepted) |
| `MODEL_NAME` | No | Model ID (default: `claude-sonnet-4-6`) |

```bash
ANTHROPIC_API_KEY=sk-ant-... \
MODEL_NAME=claude-sonnet-4-6 \
cargo run -p example-accountant-workflow
```

The run uses a fixed sample CSV of five transactions (rent, a client payment, a subscription, a contractor invoice). At each gate the program prints the pending detail and prompts `[y/N]` on stdin — reject anything to see `ApprovalDecision::Rejected` propagate back through the agent.

## How it's built

One `Agent` is created with `SkillMode::Open` and explicit skill binding (`create_session_with_skill("user", "extract-transactions")`); the router never runs since each phase is entered explicitly via `NEXT_SKILL`.

The `HitlPolicy` is derived from the loaded skill registry at startup:

```rust
for skill in reg.eligible(&SkillMode::Open) {
    if skill.phase_gate { policy.skill_phase_gates.insert(skill.id.clone()); }
    if !skill.hitl_tools.is_empty() { policy.skill_tool_gates.insert(...); }
    if !skill.checkpoints.is_empty() { policy.skill_checkpoints.insert(...); }
}
```

`run_loop` in `main.rs` drives the session through all three phases with two branches per iteration:

1. **Normal invoke** (`agent.invoke`) — until a gate fires or the phase produces final output.
2. **Resume** (`agent.resume`) — after a human decision is collected, continues the same session with `(approval_id, decision)`.

After each `Done` output, `advance_phase` is called to look for a `NEXT_SKILL` transition:

- `PhaseAdvanceResult::Advanced(transition)` — no phase gate on this skill; `transition.deliverable` becomes the next phase's input immediately.
- `PhaseAdvanceResult::Pending { approval_id }` — phase-gated; the deliverable is shown for review, and the next phase only starts once the phase gate is approved via `resume`.
- `None` — terminal output (`submit-to-ledger` never emits `NEXT_SKILL`); the loop prints the final result and returns.

`AgentOutput::Interrupted { kind, .. }` covers the other two gate types:

- `InterruptKind::SkillCheckpoint` — fired by `request_checkpoint("draft_ready", payload)` inside `prepare-journal-entry`.
- `InterruptKind::ToolApproval` — fired by the `ledger_post` call inside `submit-to-ledger`, since `ledger_post` is listed under that skill's `hitl_tools`.

`InterruptKind::PhaseGate` is unreachable from `AgentOutput::Interrupted` — phase gates always surface through `advance_phase`'s `PhaseAdvanceResult::Pending` instead.

See the repo-level [HITL section in README.md](../../README.md#human-in-the-loop-hitl) and [DEVELOPMENT.md](../../DEVELOPMENT.md#using-human-in-the-loop-hitl) for the full gate/type reference.
