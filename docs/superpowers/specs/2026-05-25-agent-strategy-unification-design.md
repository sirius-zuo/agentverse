# Agent-Strategy Unification Design

**Date:** 2026-05-25
**Scope:** Unify LlmRunner, strategies, and Agent into a single coherent hierarchy. Fix the gap where strategies bypassed LlmRunner and owned resources that belong to Agent.

---

## Goals

- `Agent` is the single access point for all LLM calls.
- `LlmRunner` is a pure message-to-response layer with no prompt logic.
- Strategies use `LlmRunner` for every LLM call; they never touch `ConnectionManager` directly.
- `PromptRegistry`, `ToolRegistry`, and `Memory` are owned by `Agent` and shared (via `Arc`) with the strategy.
- `avs-server` is a library that example binaries optionally mount. It depends on `avs-agent`; `avs-agent` never depends on `avs-server`.
- Every example is an Agent binary. No example constructs a strategy directly.

---

## Component Overview

```
avs-core
  ├── ConnectionManager   (HTTP + retry + circuit breaker)
  ├── LlmRunner           (messages → response, owns ConnectionManager only)
  ├── RunStrategy trait   (stays in avs-core, no circular deps)
  ├── PromptRegistry
  ├── ToolRegistry
  └── Memory traits

avs-react    ──┐
avs-plan    ───┤  each implements RunStrategy
avs-router  ───┘  each holds: Arc<LlmRunner>, Arc<PromptRegistry>,
                               Arc<ToolRegistry>, Arc<Mutex<dyn Memory>>

avs-strategy  (new umbrella crate)
  ├── re-exports RunStrategy trait
  ├── re-exports strategy types
  └── build(kind, runner, prompts, tools, memory) → Arc<dyn RunStrategy>

avs-agent
  ├── depends on: avs-core, avs-session, avs-strategy
  └── Agent {
        runner:   Arc<LlmRunner>
        tools:    Arc<ToolRegistry>
        prompts:  Arc<PromptRegistry>
        memory:   Arc<Mutex<dyn Memory>>
        sessions: Arc<SessionManager>
        strategy: Arc<dyn RunStrategy>
      }

avs-agent
  ├── depends on: avs-core, avs-session, avs-strategy
  ├── optional `http` feature: axum, tower, tower-http, reqwest
  │     HTTP server code compiled into avs-agent under this feature
  └── Agent {
        runner, tools, prompts, memory, sessions, strategy
        // Agent::new(..., enable_http_server: bool)
        // If enable_http_server is true, reads HOST/PORT from env and
        // spawns the HTTP server as a tokio::spawn background task
      }

example-hello-agent, example-web-search-agent, ...
  └── each is a binary that builds an Agent:
        agent.invoke_stateless(input)        — console/single-turn examples
        Agent::new(..., true) + ctrl_c wait  — example-http-agent only
      No example imports avs-server or calls avs_server::serve()
```

---

## Key Decisions

### LlmRunner is a pure message dispatcher

`LlmRunner` owns only `Arc<ConnectionManager>`. `PromptRegistry` is removed from it entirely.

```rust
pub struct LlmRunner {
    connection: Arc<ConnectionManager>,
}

pub async fn invoke(&self, messages: Vec<Message>) -> Result<GenerateResponse>
```

`LlmRunner` has no knowledge of prompts, tools, or session state. It takes a finished message list and returns a response. All prompt rendering happens upstream in the strategy or Agent.

### Agent owns all resources; strategies share via Arc

`Agent` constructs and owns `LlmRunner`, `ToolRegistry`, `PromptRegistry`, `Memory`, and `SessionManager`. Strategies hold `Arc` clones of the first four — they use them but do not configure or replace them.

"Agent owns" means: Agent decides which backend, constructs it, holds the primary reference. Swapping the strategy leaves tools, prompts, and memory unchanged.

### Strategies are non-generic

Strategy crates drop the `M: Memory` generic parameter. `Arc<Mutex<dyn Memory>>` replaces `Arc<Mutex<M>>`. This makes strategy types concrete with no unresolved type parameters, enabling `dyn RunStrategy` on `Agent`.

### Memory is used at two levels

- **Agent level**: primes initial context from long-term memory before calling the strategy; stores the final turn after the strategy returns.
- **Strategy level**: multi-step strategies (Plan, Hierarchical) query memory per-step to augment intermediate prompts. The strategy uses the shared `Arc<Mutex<dyn Memory>>` it received at construction.

### avs-server is internal to avs-agent

HTTP server capability is compiled into `avs-agent` under an optional `http` cargo feature. There is no separate public-facing `avs-server` library API. The `avs-server` crate code is absorbed into `avs-agent/src/http/`.

`Agent::new()` accepts `enable_http_server: bool`. When true, the constructor reads `HOST` and `PORT` env vars, builds the axum `Router`, and spawns `tokio::spawn(axum::serve(listener, router))` — a background task. The constructor returns immediately; the HTTP server runs concurrently.

The dependency direction is flat — no circular dependency:

```
example-http-agent
  └── agentverse-agent (features = ["http"])
        └── avs-agent/src/http/ (axum routes — compiled in, not a separate crate)
```

`Agent` has full knowledge of its own HTTP surface but nothing depends on `avs-server` as an external crate. Console examples call `agent.invoke_stateless()` directly. `example-http-agent` creates `Agent::new(..., enable_http_server: true)` then awaits a shutdown signal — it has no other HTTP-specific code.

### avs-strategy factory

`avs-strategy` is the single dependency `avs-agent` needs for strategy selection. It owns a `build()` factory and re-exports all strategy types. `avs-agent` never imports `ReActStrategy` or `PlanStrategy` directly.

### No Agent::run()

`Agent` has no `run()` method. The agent lifecycle is:
1. `Agent::new(...)` — builds all resources; if `enable_http_server = true`, spawns the HTTP server in a background task and returns
2. Callers invoke `agent.invoke_stateless(input)` or `agent.invoke(user_id, session_id, input)` directly
3. For HTTP examples, the binary keeps the process alive (e.g., `tokio::signal::ctrl_c().await`) — there is no agent-level event loop to drive

---

## Agent Structure

```rust
pub struct Agent {
    runner:   Arc<LlmRunner>,
    tools:    Arc<ToolRegistry>,
    prompts:  Arc<PromptRegistry>,
    memory:   Arc<Mutex<dyn Memory>>,
    sessions: Arc<SessionManager>,
    strategy: Arc<dyn RunStrategy>,
}

// Constructor — enable_http_server is a flag, not stored as a field.
// If true, Agent reads HOST/PORT from env vars and spawns the HTTP server
// as a background tokio task before returning.
pub fn new(
    runner:              Arc<LlmRunner>,
    tools:               Arc<ToolRegistry>,
    prompts:             Arc<PromptRegistry>,
    memory:              Arc<Mutex<dyn Memory>>,
    store:               Arc<dyn SessionStore>,
    strategy:            Arc<dyn RunStrategy>,
    enable_http_server:  bool,   // if true, reads HOST/PORT and spawns HTTP task
) -> Arc<Self>
```

### `Agent::invoke` (session-aware)

```rust
pub async fn invoke(
    &self,
    user_id: &str,
    session_id: SessionId,
    input: &str,
) -> Result<String, AgentError>
```

Orchestrates:
1. `sessions.assert_owner(user_id, session_id)`
2. `sessions.load_messages(session_id)` → session history
3. `memory.prime_context(input)` → relevant long-term context
4. Render system prompt via `prompts`
5. Assemble: `[system, long_term_context, history, user_input]`
6. `strategy.run(messages)` → response string
7. `sessions.append_turn(user_msg, assistant_msg)`
8. `memory.store(user_msg, assistant_msg)`
9. Return response

### `Agent::invoke_stateless`

```rust
pub async fn invoke_stateless(&self, input: &str) -> Result<String, AgentError>
```

No session, no history. Used by the HTTP `/invoke` route and single-turn examples. Steps 1–2 and 7–8 are skipped; memory priming (step 3) is optional.

---

## RunStrategy Trait

```rust
#[async_trait]
pub trait RunStrategy: Send + Sync {
    async fn run(&self, messages: Vec<Message>) -> Result<String, StrategyError>;
}
```

The strategy receives the fully assembled message list from `Agent`. It returns a final answer string. Internal loop steps, tool calls, and intermediate LLM calls are hidden from `Agent`.

---

## Strategy Constructors (new form)

All strategy crates replace `Arc<ConnectionManager>` with `Arc<LlmRunner>` and adopt `dyn Memory`:

```rust
// Before
ReActStrategy::new(
    prompt_registry: Arc<PromptRegistry>,
    model: Arc<ConnectionManager>,
    tools: ToolRegistry,
    memory: Arc<Mutex<M>>,
    max_iterations: usize,
)

// After
ReActStrategy::new(
    runner:  Arc<LlmRunner>,
    prompts: Arc<PromptRegistry>,
    tools:   Arc<ToolRegistry>,
    memory:  Arc<Mutex<dyn Memory>>,
    max_iterations: usize,
)
```

`ToolRegistry` changes from owned to `Arc`-shared. Memory drops the generic. All strategy structs follow the same pattern.

---

## avs-strategy Crate

```rust
pub use agentverse::RunStrategy;
pub use agentverse_react::ReActStrategy;
pub use agentverse_plan::{PlanStrategy, HierarchicalStrategy};
pub use agentverse_router::StrategyRouter;

pub enum StrategyKind {
    React,
    Plan,
    Hierarchical,
    Router,
}

pub fn build(
    kind: StrategyKind,
    runner: Arc<LlmRunner>,
    prompts: Arc<PromptRegistry>,
    tools: Arc<ToolRegistry>,
    memory: Arc<Mutex<dyn Memory>>,
    max_iterations: usize,
) -> Arc<dyn RunStrategy>
```

---

## avs-server Changes

- `avs-server` code (routes, auth, config, aether client) is absorbed into `avs-agent` under `src/http/`.
- `avs-agent/Cargo.toml` gains an optional `http` feature that adds axum, tower, tower-http, reqwest as dependencies.
- `SessionState` struct is removed. `AppState` is removed. Routes accept `Arc<Agent>` directly.
- `session_agent` variable is eliminated.
- `avs-server` as an independent workspace crate is removed from the workspace.
- All routes (`/invoke`, `/sessions/*`, `/aether/invoke`) go through one `Arc<Agent>`.
- `/invoke` calls `agent.invoke_stateless(input)`.
- `/sessions/:id/messages` calls `agent.invoke(user_id, session_id, input)`.
- `Agent::new(..., enable_http_server: true)`:
  1. Reads `HOST` (default `"0.0.0.0"`) and `PORT` (default `3000`) from env vars.
  2. Builds the axum `Router` internally.
  3. Calls `tokio::spawn(async move { axum::serve(listener, router).await })`.
  4. Returns the `Agent` — the HTTP server runs as a detached background task.
- `avs-agent` never depends on `avs-server` as an external crate. No circular dependency.

---

## Examples

Every example is rewritten as an Agent binary. Direct strategy and `ConnectionManager` construction is removed.

| Example | Strategy | Tools | Entry point |
|---|---|---|---|
| `example-hello-agent` | React | Calculator, DateTime | `agent.invoke_stateless(input)` in stdin loop |
| `example-react-calculator` | React | Calculator | `agent.invoke_stateless(input)` |
| `example-web-search-agent` | Plan | WebSearch | `agent.invoke_stateless(input)` |
| `example-anthropic-react` | React | Calculator | `agent.invoke_stateless(input)` |
| `example-code-review-agent` | Hierarchical | FileSearch, ShellTool | `agent.invoke_stateless(input)` |
| `example-slack-hr-assistant` | React | (integration-driven) | `agent.invoke_stateless(input)` |
| `example-http-agent` | React | Calculator, DateTime | `Agent::new(..., true)` then `ctrl_c().await` |

Each example carries agent config via env vars. `main.rs` creates Agent and calls `agent.invoke_stateless()` (console examples). `example-http-agent` passes `enable_http_server: true` — the Agent spawns the HTTP server internally and the binary just waits for a shutdown signal. No example imports `agentverse-server`. No example calls `agent.run()`.

---

## Data Flow: Agent::invoke

```
binary / HTTP route
    │
    ▼
Agent::invoke(user_id, session_id, input)
    │
    ├─ SessionManager::assert_owner
    ├─ SessionManager::load_messages       ──► SQLite / PostgreSQL
    ├─ Memory::prime_context               ──► LanceDB / pgvector
    ├─ PromptRegistry::render("system")
    ├─ assemble message list
    │
    ├─ RunStrategy::run(messages)
    │       │
    │       ├─ PromptRegistry::render(strategy prompts)
    │       ├─ Memory::query (per-step, multi-step strategies)
    │       ├─ LlmRunner::invoke(messages)
    │       │       └─ ConnectionManager::generate
    │       │               ├─ circuit breaker check
    │       │               ├─ ModelProvider::build_request
    │       │               ├─ reqwest::Client POST
    │       │               └─ ModelProvider::parse_response
    │       ├─ ToolRegistry::execute (tool calls)
    │       └─ repeat until answer
    │
    ├─ SessionManager::append_turn         ──► SQLite / PostgreSQL
    ├─ Memory::store                       ──► LanceDB / pgvector
    └─ return response string
```

---

## Non-Goals (this design)

- HITL states — separate design
- Per-session strategy switching — deferred
- Dynamic tool registration at runtime — deferred
- Streaming responses — deferred
- Multi-agent orchestration — separate design
