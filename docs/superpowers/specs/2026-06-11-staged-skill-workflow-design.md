# Staged Skill Workflow Examples

**Date:** 2026-06-11
**Status:** Approved

## Overview

Two new examples that demonstrate a single agent executing a multi-stage workflow where each stage is bound to a dedicated skill. The examples also serve as a showcase of all three reasoning strategies — ReAct, Plan, and Hierarchical — used in their natural contexts rather than defaulting to ReAct everywhere.

### Pattern A — Self-directing chain (`doc-pipeline`)

Each skill declares its own successor via a `NEXT_SKILL: <name>` directive on the last line of its output. `main.rs` contains no hardcoded stage names — it runs a loop until a stage emits no directive. The chain topology lives entirely in the skills.

### Pattern C — Coordinator dispatch (`support-router`)

A coordinator agent (using `StrategyKind::Plan`) reads the request and emits a native plan JSON. `main.rs` parses the plan and invokes each step with the specified skill, threading the previous step's output as context into the next.

---

## Example 1: `doc-pipeline`

### Purpose

Show Pattern A: skills form a self-directing chain. The coordinator skill does not exist in application code — skills decide the next stage themselves.

### Directory structure

```
examples/doc-pipeline/
├── Cargo.toml
├── src/
│   └── main.rs
└── skills/
    └── system/
        ├── extractor/
        │   └── SKILL.md
        ├── analyzer/
        │   └── SKILL.md
        └── summarizer/
            └── SKILL.md
```

### Stages

| Stage | Strategy | Tool | Responsibility |
|---|---|---|---|
| `extractor` | React | `find_dates(text) → Vec<String>` | Extract key facts, entities, and dates from raw document |
| `analyzer` | React | `count_mentions(term, text) → u32` | Identify patterns, significance, and relationships in extracted facts |
| `summarizer` | React | `word_count(text) → u32` | Write executive summary + bullet points; iterate until under word limit |

Each tool is a mock stub in `agentverse-demo-tools` with deterministic fake return values. No external calls.

### SKILL.md protocol

Each non-terminal skill ends its output with exactly:
```
NEXT_SKILL: <name>
```

The summarizer is terminal — it emits no directive.

### Runtime loop (`main.rs`)

```
current_skill = "extractor"
input = raw_document (from CLI arg or stdin)

loop:
  output = agent.invoke_stateless_with_skill(current_skill, input)
  (next_skill, clean_output) = parse_next_skill(output)
  if next_skill is None:
    print clean_output
    break
  input = clean_output
  current_skill = next_skill
```

`parse_next_skill(output: &str) -> (Option<String>, String)` — scans the last non-empty line for `NEXT_SKILL: <name>`, returns the name and the output with that line stripped.

### Agent configuration

One `Agent` instance, `SkillMode::Open`, `SkillConfig` pointing at `skills/`. All three mock tools registered in the tool registry (each stage only uses its own tool; unused tools are declared but ignored by the skill's instructions).

### Error handling

- Unknown `NEXT_SKILL` target: `eprintln!("unknown skill: {name}")` + `process::exit(1)`
- Cycle guard: if the same skill name appears twice in one run, exit with error

---

## Example 2: `support-router`

### Purpose

Show Pattern C: a coordinator produces a plan and specialist agents execute steps. Also demonstrates all three strategies in one example: Plan (coordinator), Hierarchical (billing), React (tech-support, account-mgmt).

### Directory structure

```
examples/support-router/
├── Cargo.toml
├── src/
│   └── main.rs
└── skills/
    └── system/
        ├── coordinator/
        │   └── SKILL.md
        ├── billing/
        │   └── SKILL.md
        ├── tech-support/
        │   └── SKILL.md
        └── account-mgmt/
            └── SKILL.md
```

### Agents and strategies

| Agent | Strategy | Role |
|---|---|---|
| `coordinator_agent` | `React` (no tools) | reads request, emits JSON plan with skill assignments; React with zero tools = one-shot LLM call; `Plan` cannot be used here because `PlanStrategy` both generates and executes its plan rather than emitting it for external dispatch |
| `specialist_agent` | varies by skill (see below) | executes one plan step |

### Specialist skills and strategies

| Skill | Strategy | Tools | Why Hierarchical / React |
|---|---|---|---|
| `billing` | **Hierarchical** | `lookup_invoice`, `check_refund_eligibility` | Three distinct reasoning steps — look up invoice, check eligibility, draft response — each a mini react loop. Hierarchical shows that a single step can itself be a full reasoning chain. |
| `tech-support` | React | `check_service_status` | Single tool call + answer. No sub-decomposition. |
| `account-mgmt` | React | `get_account_details` | Same — one lookup, one response. |

### Coordinator output format

The coordinator skill instructs the LLM to output **only** valid JSON:

```json
[
  {"skill": "billing", "task": "Check whether the user was double-charged on invoice #1042"},
  {"skill": "tech-support", "task": "Confirm whether the API outage affected the user's region"}
]
```

No prose, no markdown fences. 1–3 steps.

### Runtime (`main.rs`)

```
plan = coordinator_agent.invoke_stateless_with_skill("coordinator", request)
steps = parse_plan(plan)   // deserialize JSON → Vec<Step>

context = ""
for step in steps:
  input = format!("Task: {}\n\nPrevious context:\n{}", step.task, context)
  context = specialist_agent_for(step.skill).invoke_stateless(input)

print context
```

Three agent instances are pre-constructed at startup — one per specialist strategy — and selected by skill name at dispatch time:

```rust
let billing_agent       = Agent::new(..., StrategyKind::Hierarchical, ...);
let tech_support_agent  = Agent::new(..., StrategyKind::React, ...);
let account_mgmt_agent  = Agent::new(..., StrategyKind::React, ...);
```

Each specialist agent is constructed with `SkillMode::Open` and the full mock tool registry. Skill binding is done via `create_session_with_skill(skill_name)` per invocation.

### Mock tools

Three new structs added to `agentverse-demo-tools`:

| Tool | Returns |
|---|---|
| `LookupInvoice` | Hardcoded invoice: id=1042, amount=$99, date=2026-06-01, status=paid |
| `CheckRefundEligibility` | Always returns `eligible: true, reason: "within 30-day window"` |
| `CheckServiceStatus` | Returns `degraded` in region `us-east-1` to produce an interesting response |
| `GetAccountDetails` | Returns fake account: plan=Pro, seats=5, renewal=2026-12-01 |

### Error handling

- Coordinator output is not valid JSON: `eprintln!` + `process::exit(1)`
- Plan step names an unknown skill: `eprintln!` + `process::exit(1)`

---

## Shared conventions

- Both examples accept their input as a CLI argument: `cargo run -p example-doc-pipeline -- "raw document text"`
- Both use `ANTHROPIC_API_KEY` + `MODEL_NAME` env vars (same as other examples)
- Mock tools live in `agentverse-demo-tools`; the three new `support-router` tools are additive — existing tools are not modified
- No integration runtime (`IntegrationRuntime`) — these are pure CLI examples
- No session persistence — `SqliteSessionMemory::new("sqlite::memory:")`

---

## Strategy showcase summary

| Strategy | Where used | Why it fits |
|---|---|---|
| `React` | All `doc-pipeline` stages; `tech-support`, `account-mgmt` specialists | Tool-using stages where the agent iterates (observe → think → act) before producing output |
| `Plan` | `support-router` coordinator | Agent's sole job is to produce a structured plan — strategy and purpose align directly |
| `Hierarchical` | `billing` specialist | Step itself requires multi-step decomposition: lookup → eligibility check → draft, each a sub-chain |
