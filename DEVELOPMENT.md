# AgentVerse Developer Guide

Complete guide for developing, testing, and deploying agents with AgentVerse.

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Development Setup](#development-setup)
- [Creating a Custom Agent](#creating-a-custom-agent)
- [Using the Skill System](#using-the-skill-system)
- [Using Human-in-the-Loop (HITL)](#using-human-in-the-loop-hitl)
- [Multi-LLM Provider Configuration](#multi-llm-provider-configuration)
- [Writing Tools](#writing-tools)
- [Using MCP (Model Context Protocol)](#using-mcp-model-context-protocol)
- [Using the SubAgent Runtime](#using-the-subagent-runtime)
- [Prompt Engineering](#prompt-engineering)
- [Testing Strategies](#testing-strategies)
- [Deploying Agents](#deploying-agents)
- [Adding Long-Term Memory](#adding-long-term-memory)
  - [Retention and Data Deletion](#retention-and-data-deletion)
- [Integrating External Systems](#integrating-external-systems)
- [Debugging & Observability](#debugging--observability)
- [Quick Reference](#quick-reference)

---

## Architecture Overview

AgentVerse is a modular Rust framework organized as a Cargo workspace:

```
AgentVerse/
├── avs-core/              # LlmRunner, Config, ProviderConfig, PromptRegistry, Memory + Tool traits
├── avs-agent/             # Agent: single LLM access point; optional HTTP sidecar (feature = "http")
├── avs-skill/             # Skill system: SKILL.md parser, SkillRegistry, SkillRouter, SkillMode, SkillConfig
├── avs-hitl/              # Human-in-the-loop: HitlPolicy, ApprovalQueue (InMemoryQueue, SqliteQueue), HitlContext, RequestCheckpointTool
├── avs-strategy/          # build() factory + StrategyKind enum; re-exports all strategies
├── avs-session/           # Session lifecycle: Session model, SessionManager (storage types re-exported from avs-memory)
├── avs-subagent/          # Subagent runtime: SubAgentExecutor, SubAgentSpec, Budget, SubAgentHandle, SubAgentTool
├── avs-logging/           # avs_logging::init() (RUST_LOG / LOG_FORMAT)
├── avs-eval/              # Eval harness: deterministic scaffold + judge-based quality regression tests
├── avs-test-utils/        # Dev-dependency only: shared SessionMemory conformance suite + agent test helpers
├── avs-react/             # ReAct strategy loop
├── avs-plan/              # Plan-and-Execute + Hierarchical strategies
├── avs-router/            # Dynamic strategy routing
├── avs-tools/             # Built-in tools (Calculator, DateTime, FileSearch, HttpClient, WebSearch, ShellTool)
├── avs-mcp/               # MCP client for external tool servers
├── avs-guardrails/        # Security: prompt injection, output filtering, rate limiting
├── avs-integration/       # IntegrationRuntime with Slack, console connectors
├── avs-memory/            # Working (CacheMemory), session (SqliteSessionMemory), and long-term (VectorLongtermMemory via Embedder/VectorStore) memory tiers
├── avs-memory-lancedb/    # LanceDB VectorStore implementation
├── avs-memory-pgvector/   # pgvector VectorStore implementation + PostgresSessionMemory
└── examples/
    ├── hello-agent/        # Open-mode REPL; Extend pattern (system/ + user/ skills)
    ├── react-calculator/   # Multi-step ReAct with Calculator
    ├── web-search-agent/   # Constrained-mode web search; Shadow pattern (user/ overrides system/)
    ├── anthropic-react/    # Anthropic Claude with prompt caching
    ├── code-review-agent/  # Explicit skill binding; Hierarchical planning with FileSearch + ShellTool
    ├── slack-hr-assistant/ # IntegrationRuntime Slack/console bot
    ├── http-agent/         # Agent with enable_http_server=true
    ├── mcp-demo/           # Full MCP round-trip: McpServer + McpCatalogSource + agent
    ├── demo-tools/            # Library: 13 MCP-exposed domain tools used by staged-skill examples
    ├── project-feasibility/   # Programmatic multi-agent pipeline (SubAgentExecutor::spawn + synthesis)
    ├── business-report/       # LLM-driven multi-agent via business-report skill + SubAgentTool
    ├── doc-pipeline/          # Pattern A: self-directing skill chain (extractor→analyzer→summarizer; ReAct+Plan+ReAct)
    ├── support-router/        # Pattern C: coordinator dispatch (coordinator plans, specialists execute; React+Hierarchical+React)
    └── accountant-workflow/   # Pattern A + HITL: checkpoint, phase-gate, and tool-call approval gates
```

### Key Concepts

| Concept | Crate | Description |
|---------|-------|-------------|
| **Agent** | `agentverse-agent` | Single LLM access point — composes `LlmRunner`, strategy, `SessionManager`, memory layers, and optional `SkillConfig`. Constructed only via `AgentBuilder`; there is no direct constructor. |
| **AgentBuilder** | `agentverse-agent` | `Agent::builder(runner, tools, prompts, session_memory, strategy)` returns this; chain `.with_http_server()`, `.with_longterm_memory(...)`, `.with_skills(...)`, `.with_hitl(...)`, `.with_subagent_executor(...)`, `.with_cleanup_config(...)`, then `.build() -> Arc<Agent>` |
| **SkillConfig** | `agentverse-agent` | Wraps `SkillRegistry`, `SkillMode`, routing threshold, and precomputed caches; constructed via `SkillConfig::load` |
| **SkillMode** | `agentverse-agent` | `Open` (all skills eligible) or `Constrained(ids)` (allowlist); also re-exported from `agentverse-skill` |
| **SkillRouter** | `agentverse-skill` | Keyword-overlap scorer; binds a skill to a session on first invoke above threshold |
| **HitlConfig** | `agentverse-agent` | `{ policy: HitlPolicy, queue: Arc<dyn ApprovalQueue> }`; passed to `AgentBuilder::with_hitl` to enable HITL gates |
| **CleanupConfig** | `agentverse-agent` (`workers` module) | `{ message_retention, session_retention, poll_interval }`; passed to `AgentBuilder::with_cleanup_config` to override the default 24h/30-day/5-min retention windows |
| **HitlPolicy** | `agentverse-hitl` | Declares which tools/skills/phases require approval: `global_tool_blocklist`, `skill_tool_gates`, `skill_phase_gates`, `skill_checkpoints` |
| **ApprovalQueue** | `agentverse-hitl` | Trait for approval storage/resolution: `submit`, `resolve`, `poll`, `sweep_expired`; built-in `InMemoryQueue` and `SqliteQueue` |
| **AgentOutput** | `agentverse-agent` | `Done(String)` or `Interrupted { approval_id, kind }`; returned by `invoke`/`resume` when a HITL gate fires |
| **StrategyKind** | `agentverse-strategy` | Enum selecting the orchestration loop; `build()` constructs an `Arc<dyn RunStrategy>` |
| **LlmRunner** | `agentverse` | Renders prompts and calls model providers |
| **Config** | `agentverse` | Provider settings (model name, API key, base URL) |
| **PromptRegistry** | `agentverse` | Template engine (Minijinja) + example storage |
| **Tool** | `agentverse` | `Tool` trait with associated `type Args: JsonSchema + DeserializeOwned`; `ErasedTool` for object-safe registry dispatch |
| **WorkingMemory** | `agentverse-memory` | Layer-1 in-process cache trait; `CacheMemory` (TTL-evicted, 300 s default) is the built-in impl, override via `AgentBuilder::with_working_memory(wm)` |
| **SessionMemory** | `agentverse-memory` (re-exported by `agentverse-session`) | Layer-2 durable conversation transcript; `SessionManager` wraps it with ownership checks |
| **LongtermMemory** | `agentverse-memory` | Layer-3 cross-session knowledge store; opt-in via `AgentBuilder::with_longterm_memory(store)`. No deletion capability exists on this trait anywhere in agentverse — that data's retention is explicitly out of scope, see [Retention and Data Deletion](#retention-and-data-deletion) |
| **Embedder** / **EmbedderRegistry** | `agentverse-memory` | Text-to-vector providers behind a name-keyed factory registry (mirrors `ProviderRegistry`): `"openai"` (any OpenAI-compatible `/embeddings` endpoint; `api_key` optional when `base_url` is set, so local Ollama/llama.cpp works keyless) and `"gemini"` |
| **VectorStore** | `agentverse-memory` | Embedding storage/ANN-search trait behind `LongtermMemory`; impls: `LanceDbVectorStore` (`agentverse-memory-lancedb`, dev) and `PgVectorStore` (`agentverse-memory-pgvector`, production). Both are user-scoped and use cosine distance (relevance = 1/(1+distance)) |
| **VectorLongtermMemory** | `agentverse-memory` | The shipped `LongtermMemory` impl: `Embedder` + `VectorStore` + `ScoreWeights` (score = α·recency + β·importance + γ·relevance; defaults 0.25/0.25/0.5, 7-day recency half-life) |
| **RunStrategy** | `agentverse` | Trait implemented by all strategies; pure `Vec<Message> → String`, no memory coupling |
| **SubAgentExecutor** | `agentverse-subagent` | Orchestrates isolated worker agents; built alongside `Agent`, then passed to `AgentBuilder::with_subagent_executor(...)` for automatic `spawn_subagent` registration or registered directly with `register_tool` |
| **SubAgentSpec** / **Budget** | `agentverse-subagent` | Describes one worker: objective, system prompt, allowed tools, model override, step/token/timeout budget |
| **SubAgentHandle** | `agentverse-subagent` | Returned by `executor.spawn()`; call `await_result().await` to get the result in input order |
| **ResourceContent** | `agentverse-subagent` | Named artifact (`label`, `content`) injected into a subagent's prompt via `SubAgentContext.resources` |

---

## Development Setup

### Prerequisites

- **Rust 1.75+** (MSRV) — `rustup install stable`
- **Cargo** — bundled with Rust
- **llama.cpp** (optional, for local development) — see [Local LLM Development](#local-llm-development)
- **Docker** (optional, for pgvector testing)

### Workspace Commands

```bash
# Check the entire workspace
cargo check --workspace

# Run all tests
cargo test --workspace

# Run clippy with warnings as errors (CI runs --all-targets; do the same before pushing
# signature changes, since plain `--all` misses tests/examples/benches)
cargo clippy --workspace --all-targets -- -D warnings

# Format all code
cargo fmt --all
```

### CI Fitness Checks

Two additional checks run in CI beyond the standard fmt/clippy/test/check gate — both are plain scripts, run them locally before pushing if you've added a file or a new crate dependency:

```bash
# Fails if any .rs file exceeds 600 lines, unless allowlisted at a recorded cap
# (scripts/file-size-allowlist.txt). Prevents unbounded growth of the kind that
# produced the pre-decomposition agent.rs (1,829 lines, since split into avs-agent/src/agent/*.rs).
./scripts/check-file-sizes.sh

# Fails if any crate depends on a crate in a higher architectural layer than
# itself — catches one-directional layering violations that Cargo's own
# dependency-cycle check doesn't (a cycle requires both directions; this doesn't).
./scripts/check-layering.sh
```

`cargo-deny` (licenses/advisories/bans/sources) also runs in CI but is **not** part of the local dev gate above — it has its own job. A new crate needs `license.workspace = true` in its `Cargo.toml` (every existing crate already has it) or the CI `deny` job fails after merge, since nothing in the standard local gate exercises it. Run `cargo deny check licenses` locally if you're scaffolding a new crate and want to confirm before pushing.

### Local LLM Development

For local development without API costs, run llama.cpp as an OpenAI-compatible server:

```bash
git clone https://github.com/ggerganov/llama.cpp.git
cd llama.cpp
cmake -B build && cmake --build build --config Release -n

# Start the OpenAI-compatible server
./build/bin/llama-server -m models/your-model.gguf --host 127.0.0.1 --port 9090
```

Then set your environment variables:

```bash
export MODEL_BASE_URL=http://127.0.0.1:9090/v1
export MODEL_NAME=your-model
# MODEL_API_KEY is optional when MODEL_BASE_URL points to a local endpoint
```

---

## Creating a Custom Agent

All agents follow the same pattern: construct an `Agent` with a chosen strategy via `agentverse_strategy::build`.

### Cargo.toml

```toml
[dependencies]
agentverse = { path = "path/to/avs-core" }
agentverse-agent = { path = "path/to/avs-agent" }
agentverse-strategy = { path = "path/to/avs-strategy" }
agentverse-session = { path = "path/to/avs-session" }
agentverse-logging = { path = "path/to/avs-logging" }
agentverse-tools = { path = "path/to/avs-tools" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

### Option 1: Console Agent (ReAct)

```rust
use agentverse::{Config, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
use agentverse_agent::Agent;
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::{Calculator, DateTimeTool, ToolOptions, ToolRegistry};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let base_url = std::env::var("MODEL_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
    let model_name = std::env::var("MODEL_NAME")
        .unwrap_or_else(|_| "my-model".to_string());

    let runner = Arc::new(LlmRunner::from_config(Config {
        provider: ProviderConfig::openai(model_name, api_key, Some(base_url)),
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    }).expect("runner"));

    let tools = ToolRegistry::new();  // returns Arc<ToolRegistry>
    tools.register_with_options(Calculator, ToolOptions { category: Some("math".into()), ..Default::default() });
    tools.register_with_options(DateTimeTool, ToolOptions { category: Some("utility".into()), ..Default::default() });

    let prompts = Arc::new(PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
        ..Default::default()
    }).expect("prompts"));

    let strategy = build(
        StrategyKind::React,
        Arc::clone(&runner),
        Arc::clone(&prompts),
        Arc::clone(&tools),
        10,
    );

    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:").await.expect("session memory")
    );

    // No .with_http_server()/.with_longterm_memory()/.with_skills()/.with_hitl() calls: console-only, no extras
    let agent = Agent::builder(runner, tools, prompts, session_memory, strategy).build();

    // Stateless invoke (no session history)
    match agent.invoke_stateless("What is 6 * 7?").await {
        Ok(answer) => println!("Agent: {}", answer),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

### Option 2: Agent with Session History

```rust
// Create a session for a user
let session_id = agent.create_session("alice").await?;

// Invoke with session — history is loaded, persisted, and returned
let reply = agent.invoke("alice", session_id, "What did I just say?").await?;
```

### Option 3: HTTP Agent

Add `features = ["http"]` to `agentverse-agent` in Cargo.toml and call `.with_http_server()`:

```toml
agentverse-agent = { path = "path/to/avs-agent", features = ["http"] }
```

```rust
// .with_http_server() spawns an HTTP server as a background task
// reads HOST (default 0.0.0.0) and PORT (default 3000) from env
let _agent = Agent::builder(runner, tools, prompts, session_memory, strategy)
    .with_http_server()
    .build();

// Keep the process alive
tokio::signal::ctrl_c().await.unwrap();
```

Binding a non-loopback `HOST` without `API_KEY` now aborts startup unless `ALLOW_INSECURE=true`.

### Option 4: Anthropic Claude

```rust
let runner = Arc::new(LlmRunner::from_config(Config {
    provider: ProviderConfig::anthropic(
        "claude-sonnet-4-6",
        std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY"),
    ),
    max_messages: 50,
    tools: vec![],
    prompts_dir: None,
    system_prompt: None,
}).expect("runner"));
```

### Choosing a Strategy

| `StrategyKind` | Best for |
|---|---|
| `React` | Tool-using agents, Q&A, step-by-step reasoning |
| `Plan` | Multi-step tasks that benefit from upfront planning |
| `Hierarchical` | Complex tasks decomposed into independent sub-goals |

---

## Using the Skill System

Skills give an agent focused instructions and a tool allowlist at session creation — without any code change. Each skill is a directory containing a `SKILL.md` file with YAML frontmatter and a Markdown body.

### Directory Layout

```
skills/
  system/          # Ships with the agent binary; checked in to source
    math-helper/
      SKILL.md
    datetime-helper/
      SKILL.md
    style-guide.md # Optional supporting documents (loaded into skill context)
  user/            # Operator-added at deploy time; not committed to source
    travel-advisor/
      SKILL.md
```

`system/` skills are the defaults. `user/` skills load second and can **shadow** a system skill (same `name:` field) or **extend** the agent with a new capability (different `name:`).

### SKILL.md Format

```markdown
---
name: math-helper
description: >
  Performs arithmetic and unit conversions.
  Use when the user asks to calculate, compute, add, subtract,
  multiply, or divide numbers.
version: 1.0.0
tags:
  - math
agentverse:
  tools:
    - calculator
---

You are a precise math assistant. Use the calculator tool for all
arithmetic — never compute in your head. Show your working steps clearly.
```

| Field | Required | Description |
|---|---|---|
| `name` | yes | Skill ID used for routing and explicit binding. Must match across shadow pairs. |
| `description` | yes | Plain English; used by the keyword-overlap router to match user messages. |
| `version` | no | Semver string; informational only. |
| `tags` | no | Freeform strings; not used by the router in v1. |
| `agentverse.tools` | no | List of tool `name()` values. Only tools in this list (AND registered in `ToolRegistry`) are active for the session. Empty list = no tools. Omitting the field = no restriction. |

Any additional files in the skill directory (e.g. `style-guide.md`) are loaded as supporting documents and injected into the system prompt alongside the instructions.

### Wiring Skills into an Agent

```rust
use agentverse_agent::{Agent, SkillConfig, SkillMode};

// skills/ is resolved at compile time relative to the binary's source root
let skills_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/skills");
let skills = SkillConfig::load(skills_dir, SkillMode::Open)
    .expect("skills dir not found");

// Print skill IDs at startup (precomputed — no extra lock needed)
println!("Skills loaded: {}", skills.ids.lock().unwrap().join(", "));

let agent = Agent::builder(runner, tools, prompts, session_memory, strategy)
    .with_skills(skills)
    .build();
```

### Routing Modes

**`SkillMode::Open`** — all registered skills are candidates. Routing threshold: 0.15.

```rust
SkillConfig::load(skills_dir, SkillMode::Open)?
```

**`SkillMode::Constrained(ids)`** — only the listed skill IDs are candidates. Routing threshold: 0.08 (lower because the agent is purpose-built).

```rust
SkillConfig::load(
    skills_dir,
    SkillMode::Constrained(vec!["web-search".to_string()]),
)?
```

### How Routing Works

1. On the first `invoke` for a session, the `SkillRouter` scores the user's message against each eligible skill's `id + description` using keyword overlap (what fraction of the message words appear in the target text).
2. If a skill name appears as a whole word in the message, it wins immediately regardless of threshold.
3. The highest-scoring skill above threshold binds to the session. All subsequent messages in that session use that skill's instructions and tool allowlist.
4. If no skill scores above threshold, the agent responds normally with skill summaries visible in the system prompt.

### Explicit Binding

Bypass routing entirely by calling `create_session_with_skill` before the first `invoke`:

```rust
// Skill "code-review" is active from message 1; router never runs
let session_id = agent
    .create_session_with_skill("user", "code-review")
    .await?;

let reply = agent.invoke("user", session_id, "Review src/main.rs").await?;
```

### Shadow Pattern

A `user/` skill with the same `name:` field as a `system/` skill **replaces** it in the registry. The agent code is unchanged; only the instructions and tool list differ.

```
skills/
  system/web-search/SKILL.md   # name: web-search, version 1.0.0
  user/web-search/SKILL.md     # name: web-search, version 1.1.0 (overrides system)
```

The user variant's instructions are active for all routing after `SkillConfig::load`.

### Extend Pattern

A `user/` skill with a **new** name adds capability the system skills do not cover.

```
skills/
  system/math-helper/SKILL.md
  user/travel-advisor/SKILL.md   # new skill — no system counterpart
```

`travel-advisor` becomes an eligible routing candidate for `SkillMode::Open` agents. If it declares `tools: []`, it runs as pure language generation with no tool access.

### Hot Reload

```rust
agent.reload_skills().await?;
```

Reloads `system/` and `user/` from disk. Existing live sessions are unaffected — their bound `SkillContext` was serialized at session creation. New sessions after the reload will route against the updated registry.

### Tool Filtering

The `agentverse.tools` list in SKILL.md is intersected with the tools registered in `ToolRegistry`. Only tools that appear in both lists are active for a bound session. Tools excluded by the skill are invisible to the LLM; the LLM cannot call them even if they are registered.

```
SKILL.md tools: ["file_search", "shell"]
Registered:     ["calculator", "file_search", "shell", "web_search"]
Active:         ["file_search", "shell"]
```

Tool names must match the `name()` return value of the tool struct exactly (e.g. `"web_search"` not `"WebSearch"`).

---

## Using Human-in-the-Loop (HITL)

`agentverse-hitl` pauses execution so a human can approve, reject, or modify an action before it takes effect. Gates are declared in `SKILL.md` frontmatter — adding or removing a gate is a content change, not a code change.

### Gate Types

| Gate | Declared via | `InterruptKind` variant | Fires when |
|---|---|---|---|
| Tool approval | `hitl_tools: [tool_name, ...]` in `SKILL.md`, or `HitlPolicy::global_tool_blocklist` | `ToolApproval { tool_name, args }` | The bound skill (or global blocklist) requires approval for the tool the LLM just called |
| Skill checkpoint | `checkpoints: [name, ...]` in `SKILL.md` | `SkillCheckpoint { checkpoint_name, payload }` | The LLM calls the built-in `request_checkpoint(name, payload)` tool |
| Phase gate | `phase_gate: true` in `SKILL.md` | `PhaseGate { from_skill, to_skill, deliverable }` | `Agent::advance_phase` parses a `NEXT_SKILL` transition out of a phase-gated skill |

`HitlPolicy::new()` seeds a global tool blocklist (`file_delete`, `exec_command`, `system_shutdown`, `database_delete`) that requires approval regardless of which skill is bound — user skills cannot loosen it.

### Core Types

| Type | Crate | Description |
|---|---|---|
| `HitlPolicy` | `agentverse-hitl` | `global_tool_blocklist: HashSet<String>`, `skill_tool_gates: HashMap<SkillId, HashSet<String>>`, `skill_phase_gates: HashSet<SkillId>`, `skill_checkpoints: HashMap<SkillId, Vec<String>>` |
| `ApprovalQueue` | `agentverse-hitl` | Trait: `submit(req) -> ApprovalId`, `resolve(id, decision)`, `poll(id) -> ApprovalStatus`, `sweep_expired() -> u64` |
| `InMemoryQueue` | `agentverse-hitl` | Process-local `ApprovalQueue` impl; approvals lost on restart — fine for demos and tests |
| `SqliteQueue` | `agentverse-hitl` | Durable `ApprovalQueue` impl backed by SQLite (`SqliteQueue::new(database_url)`); survives restarts |
| `HitlConfig` | `agentverse-agent` | `{ policy: HitlPolicy, queue: Arc<dyn ApprovalQueue> }`; passed to `AgentBuilder::with_hitl` |
| `ApprovalDecision` | `agentverse-hitl` | `Approved`, `Rejected { reason }`, `Modified { new_args }` |
| `ApprovalRequest` / `ApprovalStatus` | `agentverse-hitl` | Queue entry and its lifecycle state (`Pending`, `Resolved(decision)`, `Expired`) |
| `RequestCheckpointTool` | `agentverse-hitl` | Tool named `request_checkpoint`; register it whenever any loaded skill declares `checkpoints` |
| `AgentOutput` | `agentverse-agent` | `Done(String)` or `Interrupted { approval_id, kind }` — returned by `invoke` and `resume` |
| `PhaseAdvanceResult` | `agentverse-agent` | `Advanced(PhaseTransition)` or `Pending { approval_id }` — returned by `advance_phase` |
| `HitlSweepWorker` | `agentverse-agent` | Auto-spawned by `Agent::builder(...).build()` when `.with_hitl(...)` was called; polls `queue.sweep_expired()` every 60s (`HitlSweepConfig::default()`) to reject stale pending approvals |

### Deriving a Policy from Loaded Skills

The policy is usually built directly from what the loaded `SKILL.md` files declare, rather than hand-written:

```rust
let policy = {
    let reg = skills.registry.read().await;
    let mut policy = HitlPolicy::new();
    for skill in reg.eligible(&SkillMode::Open) {
        if skill.phase_gate {
            policy.skill_phase_gates.insert(skill.id.clone());
        }
        if !skill.hitl_tools.is_empty() {
            policy.skill_tool_gates.insert(skill.id.clone(), skill.hitl_tools.iter().cloned().collect());
        }
        if !skill.checkpoints.is_empty() {
            policy.skill_checkpoints.insert(skill.id.clone(), skill.checkpoints.clone());
        }
    }
    policy
};
```

### Wiring into `Agent::builder`

```rust
use agentverse_agent::agent::HitlConfig;
use agentverse_hitl::{InMemoryQueue, RequestCheckpointTool};

tools.register(RequestCheckpointTool); // required if any skill declares `checkpoints`

let queue = Arc::new(InMemoryQueue::new());
let hitl = HitlConfig {
    policy,
    queue: Arc::clone(&queue) as Arc<dyn agentverse_hitl::ApprovalQueue>,
};

let agent = Agent::builder(runner, tools, prompts, session_memory, strategy)
    .with_skills(skills)
    .with_hitl(hitl)
    .build();
```

### The Interrupt / Resume Loop

`invoke` and `resume` both return `Result<AgentOutput, AgentError>`. A tool-approval or skill-checkpoint gate surfaces as `AgentOutput::Interrupted`:

```rust
use agentverse_agent::AgentOutput;
use agentverse_hitl::ApprovalDecision;

match agent.invoke("user", session_id, input).await? {
    AgentOutput::Done(text) => { /* final answer for this turn */ }
    AgentOutput::Interrupted { approval_id, kind } => {
        // Show `kind` to a human (ToolApproval / SkillCheckpoint), collect a decision,
        // then resume the same session with it:
        let decision = ApprovalDecision::Approved; // or Rejected { reason } / Modified { new_args }
        match agent.resume("user", session_id, approval_id, decision).await? {
            AgentOutput::Done(text) => { /* ... */ }
            AgentOutput::Interrupted { .. } => { /* another gate fired — loop again */ }
        }
    }
}
```

Phase gates don't go through `AgentOutput::Interrupted` — they surface from `advance_phase`, which runs after a skill produces its final `Done` output:

```rust
match agent.advance_phase("user", session_id, &text).await? {
    Some(PhaseAdvanceResult::Advanced(transition)) => {
        // No phase gate on this skill — transition.deliverable is the next skill's input
    }
    Some(PhaseAdvanceResult::Pending { approval_id }) => {
        // Phase-gated: show the deliverable for review, then resume with the decision.
        // Approving does not itself advance the phase — resume() only resolves the
        // interrupt. Re-invoke with the deliverable as input once approved.
    }
    None => { /* terminal output — no NEXT_SKILL directive */ }
}
```

See `examples/accountant-workflow/src/main.rs` for a complete run loop that handles all three gate types across a three-phase pipeline.

### Choosing an `ApprovalQueue`

- **`InMemoryQueue`** — no persistence, no extra dependency. Use for examples, tests, and local development.
- **`SqliteQueue::new(database_url)`** — durable; survives process restarts. Use in production, or implement `ApprovalQueue` against your own approval system (e.g. a Slack workflow or ticketing queue).

---

## Multi-LLM Provider Configuration

### ProviderConfig

`ProviderConfig` is an open, registry-keyed struct — not a closed enum — so a downstream crate can add a new provider without touching `avs-core`:

```rust
pub struct ProviderConfig {
    pub name: String,                        // looked up in a ProviderRegistry by name
    pub settings: HashMap<String, String>,    // provider-specific keys (model_name, api_key, base_url, ...)
}
```

`ConnectionManager::from_config(config, &registry)` resolves `config.name` against the registry's factories and calls the matched factory with `config.settings`; an unrecognized name fails with `ModelError::UnknownProvider`, and a missing required setting fails with `ModelError::MissingSetting(setting, provider)`.

| Built-in name | Ergonomic constructor | Settings used | Structured output |
|---|---|---|---|
| `openai` | `ProviderConfig::openai(model_name, api_key, base_url)` | `model_name`, `api_key` (optional when `base_url` is set), `base_url` (optional — omit for the real OpenAI API) | Yes — `response_format: { type: "json_schema", ... }` enforced by server |
| `anthropic` | `ProviderConfig::anthropic(model_name, api_key)` | `model_name`, `api_key` | Yes — `output_config: { format: { type: "json_schema", schema } }` enforced by server |
| `gemini` | `ProviderConfig::gemini(model_name, api_key)` | `model_name`, `api_key` | No — `response_format` silently ignored; free text returned |

`ProviderConfig::custom(name, settings)` builds a config for any name, including one a caller has registered themselves (see below).

### Configuration Examples

**Local OpenAI-compatible endpoint:**
```rust
ProviderConfig::openai(
    "my-model",
    "",                                       // empty is fine for local endpoints
    Some("http://127.0.0.1:9090/v1".to_string()),
)
```

**OpenAI:**
```rust
ProviderConfig::openai(
    "gpt-4o",
    std::env::var("OPENAI_API_KEY").unwrap(),
    None,                                     // uses OpenAI default base URL
)
```

**Anthropic:**
```rust
ProviderConfig::anthropic("claude-sonnet-4-6", std::env::var("ANTHROPIC_API_KEY").unwrap())
```

**Gemini:**
```rust
ProviderConfig::gemini("gemini-pro", std::env::var("GEMINI_API_KEY").unwrap())
```

### Registering a Custom Provider

`ProviderRegistry` is a plain, name-keyed table of factories — not global state. `LlmRunner::from_config` always resolves against `ProviderRegistry::with_builtins()` internally (the three providers above), so adding a fourth provider means building a `ConnectionManager` directly against your own registry rather than going through `LlmRunner::from_config`:

```rust
use agentverse::{ConnectionManager, LlmRunner, ProviderConfig, ProviderRegistry};

let mut registry = ProviderRegistry::with_builtins();
registry.register("my-provider", Box::new(|settings| {
    // Build and return a `ResolvedProvider { provider, api_base, api_key, model_name }`
    // by reading whatever keys your provider needs out of `settings`.
    my_provider_factory(settings)
}));

let config = ProviderConfig::custom("my-provider", my_settings);
let connection = ConnectionManager::from_config(config, &registry)?;
let runner = LlmRunner::new(std::sync::Arc::new(connection));
```

No `avs-core` changes are required — this is the fix for what used to require editing a closed 3-variant enum and every `match` over it (`ConnectionManager::from_config`, `with_model`, etc.) to add a provider.

### Structured Output

`LlmRunner` exposes two call sites:

```rust
// Free-text response (all providers)
let response = runner.invoke(messages).await?;

// Constrained JSON response matching the schema (OpenAI-compatible providers only)
let schema: serde_json::Value = /* schemars-derived schema */;
let response = runner.invoke_structured(messages, schema).await?;
```

Each provider maps the schema to its own wire format:

| Provider | Wire field | Schema enforcement |
|----------|-----------|-------------------|
| `OpenAI` (and compatible) | `response_format: { "type": "json_schema", "json_schema": { "name": "response", "schema": <schema> } }` | Server-side constrained decoding (vLLM, llama.cpp, Groq, etc.) |
| `Anthropic` | `output_config: { "format": { "type": "json_schema", "schema": <schema> } }` | Server-side enforcement by the Claude API |
| `Gemini` | *(not sent)* | Not supported — free text returned |

Hard server failures (4xx/5xx) still propagate as `ModelError::ApiError` for all providers. Use `invoke_structured` for planner agents where schema compliance is required.

---

## Writing Tools

All tools implement the `Tool` trait, which requires a strongly-typed `Args` struct. The JSON schema is derived automatically from the struct via `schemars` — no manual `parameters()` JSON needed.

### Implementing Tool

```rust
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

pub struct WeatherTool;

#[derive(Deserialize, JsonSchema)]
pub struct WeatherArgs {
    /// City name
    city: String,
}

#[async_trait::async_trait]
impl Tool for WeatherTool {
    type Args = WeatherArgs;

    fn name(&self) -> &str { "weather" }
    fn description(&self) -> &str { "Get current weather for a city" }

    async fn execute(&self, args: WeatherArgs) -> ToolResult {
        Ok(json!({ "weather": format!("Sunny in {}", args.city) }))
    }
}
```

Field doc-comments become the parameter descriptions in the schema passed to the LLM. Mark optional fields with `Option<T>` — they are automatically excluded from the `required` list.

### ErasedTool and dynamic dispatch

`Tool` is not object-safe (it has an associated type). The registry stores tools as `Arc<dyn ErasedTool>`, which provides `schema() -> Value` and `execute_raw(&self, args: Value) -> ToolResult`. The blanket impl `impl<T: Tool> ErasedTool for T` is provided automatically, so you never need to implement `ErasedTool` directly (except for MCP adapters that use server-supplied schemas).

### ToolRegistry

`ToolRegistry::new()` returns `Arc<ToolRegistry>` and auto-registers `FindToolsTool`. Registration takes `&self`, so no `mut` binding is needed.

```rust
use agentverse_tools::{Calculator, DateTimeTool, ShellTool, ToolOptions, ToolRegistry, WebSearch};
use std::time::Duration;

let registry = ToolRegistry::new();
registry.register(WeatherTool);
registry.register_with_options(Calculator, ToolOptions {
    category: Some("math".into()),
    ..Default::default()
});
registry.register_with_options(DateTimeTool, ToolOptions {
    category: Some("utility".into()),
    ..Default::default()
});

// Shell tool — sandboxed subprocess execution
registry.register_with_options(
    ShellTool::new("./workspace", Duration::from_secs(30), vec!["sudo".into(), "rm".into()]),
    ToolOptions { category: Some("shell".into()), ..Default::default() },
);
```

### ActiveToolSet

`ActiveToolSet` controls which tool schemas appear in the LLM prompt for a given invocation, without removing tools from the registry (they remain executable).

```rust
use agentverse_tools::ActiveToolSet;

let mut active = ActiveToolSet::all(&registry);   // start with everything
active.deactivate(&["find_tools", "web_search"]); // hide from this turn's prompt
let schemas = active.schemas(&registry);           // filtered schema list
```

### Parallel tool dispatch

The registry executes multiple tool calls concurrently:

```rust
use agentverse::{ToolCall};

let results = registry.execute_many(vec![
    ToolCall { name: "calculator".into(), args: json!({"operation":"add","a":1,"b":2}) },
    ToolCall { name: "datetime".into(),   args: json!({}) },
]).await;
```

The ReAct strategy does this automatically when the LLM emits multiple `Action:` / `Action Input:` blocks in one response.

### BM25 keyword search

```rust
let hits = registry.search("arithmetic math", 3);
for info in hits {
    println!("{}: score={:.2}", info.name, info.score);
}
```

`FindToolsTool` wraps this for the LLM: the model can call `find_tools` with a natural-language query to discover tools dynamically.

### ShellTool

`ShellTool` runs shell commands via `sh -c`, supporting pipes and redirections.

```rust
let tool = ShellTool::new(
    "./project",
    Duration::from_secs(30),
    vec!["sudo".into(), "curl".into()],
);
```

The agent calls it with `{ "command": "cargo test -p my-crate" }`. Response is plain text: stdout + `[stderr: ...]` if non-empty + `[exit code: N]` if non-zero.

**Security note:** `workdir` sets the initial directory but does not restrict filesystem access. Pair with a blocked-command list and OS-level isolation for production use.

---

## Using MCP (Model Context Protocol)

`agentverse-mcp` lets you both consume tools from external MCP servers and expose your own tools as an MCP server.

### Consuming an MCP server

```rust
use agentverse_mcp::{McpCatalogSource, McpClient, McpTransport};

// Streamable HTTP (MCP spec 2025-03-26)
let transport = McpTransport::StreamableHttp {
    endpoint: "https://tools.example.com/mcp".parse().unwrap(),
    headers: Default::default(),
};
// Stdio — spawn a subprocess
let transport = McpTransport::Stdio {
    command: "npx".into(),
    args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
    env: [("GITHUB_TOKEN".into(), token)].into(),
};

let client = McpClient::connect(transport).await?;
let registry = ToolRegistry::new();
let n = McpCatalogSource::populate(&registry, &client).await?;
// registry now has n tools backed by the remote server
```

Discovered tools are stored as `McpToolAdapter` which implements `ErasedTool` directly using the server-supplied schema.

### Loading from TOML config

`McpServerConfig` maps directly to a TOML table. `${VAR}` placeholders in strings are expanded from environment variables at load time.

```toml
[[mcp_servers]]
name = "github"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }

[[mcp_servers]]
name = "remote"
transport = "streamable_http"
url = "https://tools.example.com/mcp"
headers = { Authorization = "Bearer ${API_KEY}" }
```

```rust
use agentverse_mcp::{McpLoader, McpServerConfig};

let configs: Vec<McpServerConfig> = toml::from_str(&config_toml)?;
McpLoader::load(&registry, &configs).await?;
```

### Serving your tools as MCP

```rust
use agentverse_mcp::McpServer;

let mut server = McpServer::new(Arc::clone(&registry));
let port = server.bind_random_port().await?;
tokio::spawn(async move { server.run().await });
// POST http://127.0.0.1:{port}/mcp accepts initialize / tools/list / tools/call
```

See `examples/mcp-demo` for a self-contained round-trip demonstration.

---

## Using the SubAgent Runtime

`agentverse-subagent` provides isolated worker agents for multi-agent pipelines. Each subagent runs its own ReAct loop with a scoped tool registry, a step/token/timeout budget, and returns a single text answer — invisible to the parent's session history.

### Core Types

| Type | Description |
|---|---|
| `SubAgentExecutor` | Cloneable orchestrator; wraps `ConnectionManager`, `ToolRegistry`, and `PromptRegistry` |
| `SubAgentSpec` | Name, objective, optional system_prompt, model override, allowed tools, `Budget` |
| `Budget` | `max_steps: usize`, `max_tokens: u32`, `timeout: Duration` |
| `SubAgentContext` | `resources: Vec<ResourceContent>`, `depth: usize` (must be 0 for root callers) |
| `ResourceContent` | `label: String`, `content: String` — appears as `### label\ncontent` in the subagent's prompt |
| `SubAgentHandle` | Returned by `spawn()`; `await_result().await` returns `Result<SubAgentResult, SubAgentError>` |
| `SubAgentResult` | `answer: String`, `usage: UsageStats`, `steps: usize` |
| `SubAgentError` | `DepthExceeded`, `StepBudgetExceeded { steps }`, `TokenBudgetExceeded { used, limit }`, `Timeout { elapsed }`, `Llm(AgentError)`, `Panic(String)` |

### Building the Executor

`SubAgentExecutor` is created alongside, not inside, `Agent`. It shares the same `ConnectionManager` and `ToolRegistry`:

```rust
use agentverse_subagent::{Budget, SubAgentContext, SubAgentExecutor, SubAgentSpec};
use std::sync::Arc;
use std::time::Duration;

let executor = SubAgentExecutor::new(
    Arc::clone(&connection_manager),
    Arc::clone(&tool_registry),   // subagents only see tools listed in allowed_tools
    Arc::clone(&prompt_registry),
);
```

### Running Subagents

**Sequential (`run`):**

```rust
let result = executor.run(&SubAgentSpec {
    name: "analyst".into(),
    objective: "Estimate the NPV for project X assuming 12% discount rate.".into(),
    system_prompt: Some("You are a financial analyst.".into()),
    model: None,   // inherit parent; or Some(ModelOverride::Alias("haiku".into()))
    allowed_tools: vec!["npv_calculator".into()],
    budget: Budget {
        max_steps: 8,
        max_tokens: 4000,
        timeout: Duration::from_secs(90),
    },
}, SubAgentContext { resources: vec![], depth: 0 }).await?;

println!("{}", result.answer);    // final answer text
println!("steps={} tokens={}", result.steps,
    result.usage.input_tokens + result.usage.output_tokens);
```

**Parallel, input order preserved (`spawn` + `await_result`):**

All three subagents start immediately via `tokio::spawn`. Awaiting handles in input order costs no extra wall-clock time — the slowest task determines total duration.

```rust
use agentverse_subagent::SubAgentHandle;

let labeled: Vec<(&str, SubAgentHandle)> = vec![
    ("Financial", executor.spawn(financial_spec, ctx.clone())),
    ("Timeline",  executor.spawn(timeline_spec,  ctx.clone())),
    ("Risk",      executor.spawn(risk_spec,       ctx.clone())),
];
for (label, handle) in labeled {
    match handle.await_result().await {
        Ok(r)  => println!("{}: {}", label, r.answer),
        Err(e) => println!("{}: FAILED — {}", label, e),
    }
}
```

**Parallel, completion order (`run_many`):**

`run_many` uses `JoinSet::join_next` and returns results as tasks complete — not in input order. Use `spawn` + `await_result` whenever labels or sequence matter.

```rust
let results = executor.run_many(tasks).await;  // Vec<Result<SubAgentResult, SubAgentError>>
```

### Chaining with ResourceContent

Pass prior results into a downstream subagent via `SubAgentContext.resources`. Resources appear as `### label` sections in the subagent's initial user message:

```rust
use agentverse_subagent::ResourceContent;

let synthesis_ctx = SubAgentContext {
    resources: vec![
        ResourceContent { label: "Financial Analysis".into(), content: financial.answer },
        ResourceContent { label: "Risk Analysis".into(),      content: risk.answer },
    ],
    depth: 0,
};
let report = executor.run(&synthesis_spec, synthesis_ctx).await?;
```

### LLM-Driven Orchestration via SubAgentTool

Register `SubAgentTool` so the LLM can call `spawn_subagent` as a tool. Pair with a `SKILL.md` that instructs the LLM when to delegate:

```rust
let executor = Arc::new(SubAgentExecutor::new(cm, tools, prompts));
let agent_tools = ToolRegistry::new();
SubAgentExecutor::register_tool(&executor, &agent_tools);   // registers "spawn_subagent"

let agent = Agent::builder(runner, agent_tools, prompts, session, strategy)
    .with_skills(skills)
    .build();
```

In `SKILL.md`, use prose — not template variables — to reference the user's input. The parser stores the body verbatim with no substitution:

```markdown
## Step 1 — spawn analyst
Call spawn_subagent with name="market-analyst". The objective should ask the analyst
to assess the market opportunity for the user's specific company/product...
```

### Model Overrides

```rust
use agentverse_subagent::ModelOverride;

spec.model = Some(ModelOverride::Alias("haiku".into()));        // → claude-haiku-4-5-20251001
spec.model = Some(ModelOverride::Alias("sonnet".into()));       // → claude-sonnet-4-6
spec.model = Some(ModelOverride::Alias("opus".into()));         // → claude-opus-4-8
spec.model = Some(ModelOverride::Id("custom-model-id".into())); // raw ID passthrough
```

### Depth Limit

Subagents cannot spawn nested subagents. `filter_by_names` always excludes `spawn_subagent` from scoped tool registries, and a depth guard in `SubAgentExecutor::run` returns `SubAgentError::DepthExceeded` as defense-in-depth. Maximum supported depth is 1.

### Testing Tools Used by Subagents

Test tool computation logic directly — no executor or LLM required:

```rust
#[tokio::test]
async fn runway_projector_breakeven() {
    let tool = RunwayProjector;
    let result = tool.execute(RunwayArgs {
        initial_funding_usd: 500_000.0,
        monthly_burn_usd: 10_000.0,
        monthly_revenue_usd: 8_000.0,
        monthly_revenue_growth_pct: 0.05,
    }).await.unwrap();
    // Month 5: revenue ≈ $9,724 ≥ $10,000? → verify break-even timing
    assert!(result["breakeven_month"].as_u64().unwrap() <= 6);
}
```

Integration tests for full subagent pipelines require a running model server (`MODEL_BASE_URL`).

---

## Prompt Engineering

AgentVerse uses a **three-layer prompt system** designed to maximize LLM prompt cache reuse.

### Two Patterns

How you configure the prompt layers depends on whether your agent needs a cross-skill baseline.

**Pattern A — prompts-primary** (recommended for multi-skill agents or any agent where safety/behavioral invariants apply across all skills)

Include a `prompts/` directory with:
- `system.j2` — cross-skill baseline only. Permitted: one-line agent identity, behavioral invariants, safety rules. Prohibited: domain logic, workflow steps, tool guidance, output formats tied to a specific skill. **Rule: if the instruction would change when switching skills, it belongs in `SKILL.md`, not `system.j2`.**
- A strategy template (`react.j2`, `hierarchical.j2`, `plan_and_execute.j2`) if the strategy requires format instructions.

`SKILL.md` is the authoritative source for everything domain-specific: persona, workflow, tool guidance, output format.

Example thin `system.j2`:
```jinja2
You are a helpful assistant. Be concise, accurate, and honest.
Do not fabricate information.
```

**Pattern B — skills-only** (for agents whose behavior is entirely defined by skills)

Use `PromptRegistry::new()` with no `prompts/` directory. `SKILL.md` carries all instructions. Demonstrates that `system.j2` is optional — `doc-pipeline` and `support-router` use this pattern.

---

### Template Roles

| Layer | File | Contains | Cache behaviour |
|---|---|---|---|
| System | `system.j2` | Cross-skill baseline: agent identity + behavioral invariants + safety rules — prepended by `[skill instructions + docs]` when a skill is active, or by skill summaries during the routing phase | Cached in the system block — paid once per session |
| Preamble | `react.j2` | Tool descriptions + format instructions + few-shot examples — **only inserted when a `prompts/` directory with a `react.j2` file is configured** (`PromptRegistry::new()` without a directory skips this layer entirely) | Inserted after the System message (first non-System position); captured by the penultimate-message cache breakpoint |
| Conversation | *(memory)* | Thought / Action / Tool Result / Answer exchanges | Volatile |

> **Skill effect on the preamble:** when a skill is active its `agentverse.tools` allowlist filters `active_tool_names` before `react.j2` is rendered, so only the skill's permitted tools appear in the preamble's tool block.

### Directory Layout

**ReAct strategy:**
```
prompts/
  system.j2              # Identity + rules
  react.j2               # Tools + format + {% if examples %}...{% endif %}
  react_examples.toml    # Few-shot examples (name: "react_examples")
```

**Plan-and-Execute:**
```
prompts/
  system.j2
  plan_and_execute.j2    # → "strategies.plan_and_execute"
  plan_examples.toml
```

**Hierarchical:**
```
prompts/
  system.j2
  hierarchical.j2        # → "strategies.hierarchical.decompose"
  hierarchical_examples.toml
```

### Template Files (.j2)

`react.j2` example:

```jinja2
Available tools:
{{ tools }}

Always respond in this exact format:

    Thought: <your reasoning>
    Action: <tool_name>
    Action Input: <json arguments>

When you have the final answer:

    Thought: <brief summary>
    Answer: <final result>

> **`Action Input:` JSON may span multiple lines.** The parser accumulates
> every non-keyword line after `Action Input:` into a single buffer and parses
> it as JSON when the next keyword (`Thought:`, `Action:`, `Observation:`,
> `Answer:`) is encountered or the response ends. Both of these are valid:
>
> ```
> Action Input: {"key": "value"}          # same line
>
> Action Input:                           # JSON on following lines
> {
>   "key": "value"
> }
> ```

{% if examples %}
Examples:
{% for example in examples %}
User: {{ example.input }}
Assistant: {{ example.output }}
{% endfor %}
{% endif %}
```

### Example Files (.toml)

```toml
# prompts/react_examples.toml
[[example]]
input = "What is 6 * 7?"
output = "Thought: I need to multiply.\nAction: calculator\nAction Input: {\"operation\": \"multiply\", \"a\": 6, \"b\": 7}"
```

### SubAgent Prompt Assembly

`SubAgentExecutor` bypasses the three-layer system entirely. It assembles messages directly in `build_initial_messages` without touching `system.j2`, `react.j2`, or skill context:

```text
[optional System message — spec.system_prompt only]
User: Objective: <spec.objective>

## Context
### <resource.label>
<resource.content>
...
```

No `system.j2` is rendered. No preamble is inserted regardless of `PromptRegistry` configuration. The ReAct format instruction must come from the `spec.system_prompt` field or from the skill body when the subagent is invoked via `SubAgentTool`.

---

## Testing Strategies

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_tool() {
        let tool = MyTool;
        assert_eq!(tool.name(), "my_tool");
        assert!(tool.description().contains("useful"));
    }
}
```

### Integration Tests

```rust
// tests/integration_test.rs
use agentverse::{Config, LlmRunner, ProviderConfig};

#[test]
fn test_runner_creation() {
    let runner = LlmRunner::from_config(Config {
        provider: ProviderConfig::openai("gpt-4", "test-key", None),
        max_messages: 10,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    });
    assert!(runner.is_ok());
}
```

### Running Tests

```bash
cargo test --workspace
cargo test -p agentverse-agent
cargo test --workspace -- --nocapture
```

### Eval Harness

The `avs-eval` crate (see the [Eval Harness README section](README.md#eval-harness)) covers two distinct testing concerns, deliberately kept separate:

1. **Deterministic scaffold regression** (`avs-eval/tests/deterministic_test.rs`) — pure-function tests with zero LLM/network calls, for the ReAct parser, the skill router, and prompt template rendering. Fixtures live in `avs-eval/fixtures/{parser,router,templates}/*.toml`.
2. **Judge-based quality regression** (`avs-eval/tests/judge_test.rs`) — runs the real `Agent`/strategy stack against a *recorded* LLM response (via `httpmock`, loaded from `avs-eval/fixtures/recordings/*.toml`), captures the output, then scores it against a rubric using a *recorded* judge-model response. Both the agent-under-test and the judge are fully mocked in every automated run — this project has no CI job, scheduled workflow, or any other automated path that ever makes a live LLM call, and this harness does not introduce the first one.

**Refreshing recordings against live models:** each recording file (`avs-eval/fixtures/recordings/<case>.toml`) holds a sequence of agent-model turns plus one judge-model turn, each just a `body_contains` matcher and a `content` string — see `scripts/refresh-judge-recordings.sh` for the exact manual procedure to re-capture these against a live model. This script is never run by CI; a developer runs it locally with real API keys, reviews the newly-captured live model output, and commits the updated recording file(s) like any other fixture change. This is intentional: it keeps a human in the loop reviewing what a live model actually said before it becomes a permanent regression expectation.

**Why judge scoring is Pass/Fail, not a numeric score:** a strict binary verdict against an explicit rubric avoids the calibration drift that numeric LLM-judge scores are prone to — a case either meets its rubric or it doesn't, with no threshold to tune.

### SessionMemory Conformance Suite

`agentverse-test-utils::session_conformance::run_conformance_suite` is a single, shared test function exercising the full `SessionMemory` trait contract. Both backends run it against a real instance of themselves:

```rust
// avs-test-utils/tests/sqlite_conformance.rs
use agentverse_session::SqliteSessionMemory;
use agentverse_test_utils::session_conformance::run_conformance_suite;

#[tokio::test]
async fn sqlite_session_store_conforms() {
    let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
    run_conformance_suite(&store).await;
}
```

The Postgres equivalent (`avs-memory-pgvector/tests/pg_conformance.rs`) reads `TEST_DATABASE_URL` and early-returns if it's unset — set it to a real Postgres instance to actually exercise it; without it, `cargo test --workspace` reports the test as passed without having run any Postgres-specific logic.

Adding a third `SessionMemory` backend means writing one `#[tokio::test]` like the one above, not duplicating test logic. This is also how the two backends are kept behaviorally identical by construction — a semantic drift between SQLite and Postgres (e.g. one cascading a delete and the other not) fails the same shared assertions on both, rather than relying on two independently-written test suites staying in sync by discipline alone.

---

## Deploying Agents

### Option 1: Standalone Console Binary

```bash
cargo build --release -p example-hello-agent
MODEL_BASE_URL=http://127.0.0.1:9090/v1 MODEL_NAME=my-model \
  ./target/release/example-hello-agent
```

### Option 2: HTTP Agent Binary

```bash
cargo build --release -p example-http-agent
ANTHROPIC_API_KEY=sk-ant-... HOST=0.0.0.0 PORT=3000 ALLOW_INSECURE=true \
  ./target/release/example-http-agent
```

Or build your own binary with `enable_http_server=true` and `features = ["http"]` — see [Option 3 in Creating a Custom Agent](#option-3-http-agent).

### Option 3: Docker

```dockerfile
FROM rust:1.75-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p example-http-agent

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/example-http-agent .
EXPOSE 3000
CMD ["./example-http-agent"]
```

```bash
docker build -t agentverse-http .
docker run -p 3000:3000 \
  -e ANTHROPIC_API_KEY=sk-ant-... \
  -e HOST=0.0.0.0 \
  -e PORT=3000 \
  agentverse-http
```

---

## Adding Long-Term Memory

Layer-3 `LongtermMemory` is opt-in. Call `.with_longterm_memory(store)` on `AgentBuilder`; omit the call to disable it entirely.

The shipped implementation is `VectorLongtermMemory` (in `agentverse-memory`): a pluggable `Embedder` plus a `VectorStore` backend. Dev wiring — local Ollama embedder (keyless, OpenAI-compatible) with a file-based LanceDB store:

```rust
use agentverse_memory::{EmbedderRegistry, VectorLongtermMemory};
use agentverse_memory_lancedb::LanceDbVectorStore;
use std::{collections::HashMap, sync::Arc};

let embedder = EmbedderRegistry::with_builtins().build(
    "openai",
    &HashMap::from([
        ("model_name".to_string(), "nomic-embed-text".to_string()),
        ("base_url".to_string(), "http://localhost:11434/v1".to_string()), // Ollama, no api_key
        ("dimensions".to_string(), "768".to_string()),
    ]),
)?;
let store = Arc::new(LanceDbVectorStore::new("./data/lancedb", "memories", 768));
let longterm = Arc::new(VectorLongtermMemory::new(embedder, store));

let agent = Agent::builder(runner, tools, prompts, session_memory, strategy)
    .with_longterm_memory(longterm)
    .build();
```

Production: same registry with `"openai"` + an API key (or `"gemini"`), and `agentverse_memory_pgvector::PgVectorStore` as the store (schema in `avs-memory-pgvector/src/migration.sql` — adjust the `vector(1536)` dimension to your embedder's). Retrieval scoring weights are tunable via `VectorLongtermMemory::with_weights(ScoreWeights { .. })`.

Any custom type implementing `agentverse_memory::LongtermMemory` (`write`/`retrieve`) also works.

On each `invoke` call the agent:
1. Retrieves the top-k scored memories (`score = α·recency + β·importance + γ·relevance`) and injects them into the system prompt.
2. Asynchronously writes the completed turn as a `LongtermRecord` (fire-and-forget, off the latency path).

Background workers (`ConsolidationWorker`, `CleanupWorker` in `avs-agent`) handle batch consolidation and retention-window cleanup independently of the per-turn write.

`LongtermMemory` exposes `write`/`retrieve` only — **no deletion method exists on this trait anywhere in agentverse.** This is a deliberate, firm design decision, not a gap: Layer-3 data may serve purposes beyond a single agent's own runtime (e.g. training corpora), so its retention policy is treated as the operator's responsibility, not this framework's. See [Retention and Data Deletion](#retention-and-data-deletion) below for what *is* deletable (Layers 1 and 2).

### Retention and Data Deletion

Two independent, unrelated cleanup concerns run on the same `CleanupWorker`, configured via `AgentBuilder::with_cleanup_config`:

```rust
use agentverse_agent::workers::CleanupConfig;
use std::time::Duration;

let agent = Agent::builder(runner, tools, prompts, session_memory, strategy)
    .with_cleanup_config(CleanupConfig {
        message_retention: Duration::from_secs(86_400),    // default: 24h
        session_retention: Duration::from_secs(2_592_000), // default: 30 days
        poll_interval: Duration::from_secs(300),           // default: 5 min
    })
    .build();
```

| Field | Governs |
|---|---|
| `message_retention` | How long a raw message survives *after* it has been consolidated into `LongtermMemory` (or is exempt, if no `LongtermMemory` is configured). Never prunes an unconsolidated message, regardless of age — weakening this to an age-only check would risk permanent, silent data loss. |
| `session_retention` | How long a session survives after it ends (`Completed`/`Interrupted`), before the whole session row — and all of its messages, via `ON DELETE CASCADE` — is deleted. |
| `poll_interval` | How often the worker runs both checks. |

Each tick: bulk-deletes ended sessions past `session_retention` first, then prunes eligible messages from whatever sessions remain — avoiding wasted per-message work on a session about to be deleted wholesale in the same tick.

`SessionMemory::list_sessions_needing_maintenance()` (used by both `ConsolidationWorker` and `CleanupWorker`) is scoped by *pending work*, not by session status — a session that has ended but still has unconsolidated messages remains visible until fully drained. (An earlier version of this scoping incorrectly filtered to `status = 'active'` only, silently stranding any session's trailing messages the moment the conversation ended — this is why the check is worth calling out explicitly here.)

For an explicit per-user delete (e.g. a "right to be forgotten" API endpoint):

```rust
agent.delete_all_user_data("alice").await?;
```

This deletes every Layer-2 (`SessionMemory`) session the user owns and evicts every matching Layer-1 (`CacheMemory`) entry. It does **not** call `assert_owner` — unlike `end_session`/`get_session`, it never takes a caller-supplied `session_id` to check against `user_id`; every session it touches comes from `list_sessions(user_id)` itself, already scoped by the trusted `user_id` parameter. It does **not** touch Layer-3 `LongtermMemory`, per the design decision above.

### SQLite database location

`SqliteSessionMemory` (Layer 2) uses a file URL like `"sqlite:agent.db"`. The file is created in the working directory where `cargo run` is invoked (typically the repo root). Most examples use `"sqlite::memory:"` — an in-process database that does not write to disk and is lost when the process exits.

To inspect data at runtime:
```bash
sqlite3 agent.db "SELECT session_id, role, content FROM messages ORDER BY sequence_num;"
# or enable sqlx trace logging for bound parameters:
RUST_LOG=sqlx=trace cargo run -p example-http-agent
```

---

## Integrating External Systems

### IntegrationRuntime

`IntegrationRuntime` is the unified connector host. Your agent provides a handler closure; the runtime calls it for each incoming event.

```rust
use agentverse_integration::{Event, IntegrationRuntime};
use std::sync::Arc;

let agent = Arc::new(Agent::builder(runner, tools, prompts, session_memory, strategy).build());
let runtime = IntegrationRuntime::from_config("agent.toml").await?;

runtime
    .run(move |event: Event| {
        let agent = Arc::clone(&agent);
        async move {
            let answer = agent
                .invoke_stateless(&event.text)
                .await
                .map_err(|e| agentverse::AgentError::Memory(e.to_string()))?;
            Ok::<Event, agentverse::AgentError>(Event { text: answer, ..event })
        }
    })
    .await?;
```

**`agent.toml` example (Slack connector):**
```toml
[integration]
input = "slack"
outputs = ["slack"]

[connector.slack]
port = 3000
bot_token_env = "SLACK_BOT_TOKEN"
signing_secret_env = "SLACK_SIGNING_SECRET"
```

If no config file is found, `IntegrationRuntime::from_config` falls back to `ConsoleConnector` — useful for local development.

---

## Debugging & Observability

### Logging Setup

All examples call `avs_logging::init()` at startup. This reads two environment variables:

| Variable | Effect |
|----------|--------|
| `RUST_LOG` | Log level filter — e.g. `info`, `debug`, `agentverse_react=trace` |
| `LOG_FORMAT` | Set to `json` for structured JSON output; omit for human-readable text |

```bash
RUST_LOG=debug cargo run -p example-hello-agent
LOG_FORMAT=json RUST_LOG=info cargo run -p example-http-agent
RUST_LOG=agentverse_react=debug cargo run -p example-hello-agent
```

### What Gets Logged

**`LlmRunner`** (every LLM call, `debug` level):
```
>>>>>>>>>> LLM PROMPT BEGIN <<<<<<<<<<
...
>>>>>>>>>> LLM RESPONSE BEGIN <<<<<<<<<<
...
```

**`ToolRegistry`** (every tool call, `debug` level):
```
Executing tool  tool="calculator" args=...
Tool result     tool="calculator" result=...
```

**`ReActStrategy`** (each loop iteration, `info` level):
```
Thought only, continuing  iteration=1
Tool executed             iteration=2 tool="calculator"
Strategy completed        iteration=3
```

### Common Debugging Scenarios

**`Config(Missing("provider.api_key is required"))` at startup:**
- This error is only raised for the `openai` provider when `base_url` is unset (i.e., real OpenAI endpoint). Set `MODEL_API_KEY` or provide `base_url`.
- For local endpoints, `api_key` can be empty as long as `base_url` is set.

**Agent not responding:**
1. Check `MODEL_BASE_URL` and `MODEL_API_KEY`.
2. Verify the model server is running: `curl $MODEL_BASE_URL/models`.
3. Enable debug logging: `RUST_LOG=debug` — you'll see the full LLM prompt.

**Tool not being called:**
1. `RUST_LOG=debug` — confirm the tool schema appears in the prompt.
2. Check that the tool name in the prompt matches `tool.name()` exactly.

---

## Quick Reference

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MODEL_BASE_URL` | `http://localhost:9090/v1` | OpenAI-compatible LLM backend |
| `MODEL_API_KEY` | *(empty)* | API key; optional for local `base_url` |
| `MODEL_NAME` | model-specific | Model identifier |
| `ANTHROPIC_API_KEY` | — | Anthropic API key |
| `HOST` | `0.0.0.0` | HTTP server bind address |
| `PORT` | `3000` | HTTP server port |
| `API_KEY` | *(unset)* | Bearer token required by all HTTP routes when set to a non-empty value. Required for non-loopback binds unless ALLOW_INSECURE=true. |
| `ALLOW_INSECURE` | *(unset)* | Explicit opt-out: serve HTTP on a non-loopback address without a non-empty API_KEY. Startup fails otherwise. |
| `RUST_LOG` | `info` | Log level filter |
| `LOG_FORMAT` | *(text)* | Set to `json` for structured JSON output |

### Key Crates and Types

| Crate | Key Types |
|-------|-----------|
| `agentverse` | `LlmRunner` (`invoke`, `invoke_structured`), `Config`, `ProviderConfig`, `ProviderRegistry`, `ConnectionManager`, `PromptRegistry`, `RunStrategy`, `Tool`, `ErasedTool`, `ToolCall`, `ToolResult`, `ModelError` |
| `agentverse-agent` | `Agent`, `AgentBuilder`, `AgentError`, `AgentOutput`, `HitlConfig`, `CleanupConfig` (in `workers`), `PhaseAdvanceResult`, `parse_phase_transition`, `SkillConfig`, `SkillMode` |
| `agentverse-skill` | `SkillRegistry`, `SkillRouter`, `SkillMode`, `SkillConfig`, `Skill`, `SkillContext`, `SkillError` |
| `agentverse-hitl` | `HitlPolicy`, `ApprovalQueue`, `InMemoryQueue`, `SqliteQueue`, `HitlContext`, `ApprovalRequest`, `ApprovalDecision`, `ApprovalStatus`, `InterruptKind`, `RequestCheckpointTool`, `HitlError` |
| `agentverse-strategy` | `build()`, `StrategyKind` |
| `agentverse-session` | `SessionManager`, `SessionId` (plus re-exports of the `agentverse-memory` session types below) |
| `agentverse-memory` | `WorkingMemory`, `CacheMemory`; `SessionMemory`, `SqliteSessionMemory`, `Session`, `SessionId`, `InterruptedState`; `LongtermMemory`, `LongtermRecord`, `ScoredMemory`, `VectorLongtermMemory`, `ScoreWeights`, `Embedder`, `EmbedderRegistry`, `VectorStore`, `VectorRecord`, `VectorHit`, `NoopVectorStore` |
| `agentverse-memory-lancedb` | `LanceDbVectorStore` |
| `agentverse-memory-pgvector` | `PgVectorStore`, `PostgresSessionMemory` |
| `agentverse-logging` | `init()` |
| `agentverse-react` | `ReActStrategy` |
| `agentverse-plan` | `PlanStrategy`, `HierarchicalStrategy` |
| `agentverse-router` | `StrategyRouter` |
| `agentverse-guardrails` | `check_prompt`, `check_output`, `RateLimiter` |
| `agentverse-tools` | `ToolRegistry`, `ActiveToolSet`, `ToolOptions`, `ExecutionMode`, `FindToolsTool`, `Calculator`, `DateTimeTool`, `FileSearch`, `HttpClient`, `ShellTool`, `WebSearch` |
| `agentverse-mcp` | `McpClient`, `McpServer`, `McpTransport`, `McpCatalogSource`, `McpLoader`, `McpServerConfig`, `McpToolAdapter`, `McpError` |
| `agentverse-subagent` | `SubAgentExecutor`, `SubAgentSpec`, `Budget`, `ModelOverride`, `SubAgentContext`, `ResourceContent`, `SubAgentHandle`, `SubAgentResult`, `SubAgentError`, `SubAgentTool` |
| `agentverse-integration` | `IntegrationRuntime`, `Event` |
| `agentverse-eval` | (test-only; no public library API consumed by other crates) |
| `agentverse-test-utils` | `dead_endpoint_agent`, `unwrap_done`, `session_conformance::run_conformance_suite` |

### ProviderConfig Struct

```rust
pub struct ProviderConfig {
    pub name: String,
    pub settings: HashMap<String, String>,
}
```

Not a closed enum — `name` is looked up against a `ProviderRegistry` at connection time (built-ins: `openai`, `anthropic`, `gemini`; extensible via `ProviderRegistry::register`). Ergonomic constructors: `ProviderConfig::openai(model_name, api_key, base_url)`, `::anthropic(model_name, api_key)`, `::gemini(model_name, api_key)`, `::custom(name, settings)`. `api_key` is validated as non-empty only for `openai` when `base_url` is unset — see [Multi-LLM Provider Configuration](#multi-llm-provider-configuration).

### ModelError Variants

```rust
pub enum ModelError {
    ApiError(String),
    Timeout(String),
    InvalidResponse(String),
    RateLimited(String),
    CircuitOpen(String),
}
```
