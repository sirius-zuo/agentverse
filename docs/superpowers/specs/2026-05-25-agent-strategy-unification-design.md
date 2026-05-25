# Agent-Strategy Unification Design

**Date:** 2026-05-25
**Scope:** Unify LlmRunner, strategies, and Agent into a single coherent hierarchy. Fix the gap where strategies bypassed LlmRunner and owned resources that belong to Agent.

---

## Goals

- `Agent` is the single access point for all LLM calls.
- `LlmRunner` is a pure message-to-response layer with no prompt logic.
- Strategies use `LlmRunner` for every LLM call; they never touch `ConnectionManager` directly.
- `PromptRegistry`, `ToolRegistry`, and `Memory` are owned by `Agent` and shared (via `Arc`) with the strategy.
- `avs-server` is a library Agent optionally starts. No binary can start without an Agent.
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
  ├── optionally depends on: avs-server (http feature)
  └── Agent {
        runner:   Arc<LlmRunner>
        tools:    Arc<ToolRegistry>
        prompts:  Arc<PromptRegistry>
        memory:   Arc<Mutex<dyn Memory>>
        sessions: Arc<SessionManager>
        strategy: Arc<dyn RunStrategy>
        http:     Option<HttpConfig>
      }

avs-server  (library only — no binary, no Agent construction)
  └── serve(agent: Arc<Agent>, config: HttpConfig) → routes

example-hello-agent, example-web-search-agent, example-http-agent, ...
  └── each is a binary that builds an Agent and calls agent.run()
      avs-agent and avs-server are never run directly
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

### avs-server is a library Agent starts

`avs-server` is a library crate with no binary and no Agent construction. It provides HTTP routes that Agent mounts when `http` config is present. Agent depends on `avs-server` (optional feature). Nothing depends on `avs-server` to create an Agent.

```
avs-server  (library — routes only)
    ↑
avs-agent   (optional Cargo feature `http` pulls in avs-server)
    ↑
example and production binaries
```

Every binary entry point is an Agent. HTTP is a capability Agent optionally starts.

### avs-strategy factory

`avs-strategy` is the single dependency `avs-agent` needs for strategy selection. It owns a `build()` factory and re-exports all strategy types. `avs-agent` never imports `ReActStrategy` or `PlanStrategy` directly.

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

### `Agent::run`

```rust
pub async fn run(&self) -> Result<(), AgentError>
```

Starts the agent. If `http` config is present, mounts routes and serves HTTP. Otherwise, runs in integration/console mode via `avs-integration`.

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

- `avs-server` is a library crate only — it has no binary and never constructs an Agent.
- `avs-agent` is a library crate only — the framework is never run directly.
- The existing `agentverse-server` binary becomes `example-http-agent`: an example that shows how to build an HTTP-serving agent using `avs-agent` with the `http` feature.
- `SessionState` struct is removed. `AppState` is removed. Routes accept `Arc<Agent>` directly.
- `session_agent` variable in `main.rs` is eliminated.
- All routes (`/invoke`, `/sessions/*`, `/aether/invoke`) go through one `Arc<Agent>`.
- `/invoke` calls `agent.invoke_stateless(input)`.
- `/sessions/:id/messages` calls `agent.invoke(user_id, session_id, input)`.

---

## Examples

Every example is rewritten as an Agent binary. Direct strategy and `ConnectionManager` construction is removed.

| Example | Strategy | Tools |
|---|---|---|
| `example-hello-agent` | React | Calculator, DateTime |
| `example-react-calculator` | React | Calculator |
| `example-web-search-agent` | Plan | WebSearch |
| `example-anthropic-react` | React | Calculator |
| `example-code-review-agent` | Hierarchical | FileSearch, ShellTool |
| `example-slack-hr-assistant` | React | (integration-driven) |

Each example carries agent config (env vars or YAML). `main.rs` creates Agent and calls `agent.run()` or `agent.invoke_stateless(input)`.

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
