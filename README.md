# AgentVerse

AgentVerse is a production-grade Rust framework structured around strict layer separation: a provider-agnostic LLM runner at the base, reasoning strategies (ReAct, Plan, Hierarchical) as pure stateless transformers above it, a session layer that maintains per-user transcript isolation, and a skill layer that controls what behavior and toolset each session activates — all composed through a single `Agent` type that enforces the boundaries between them.

What makes it different: **behavior is operator-driven, not code-driven.** Drop a `SKILL.md` file into `skills/system/` or `skills/user/` and the agent rewrites its own system prompt, restricts its active toolset, and adjusts its routing threshold — at runtime, without a redeploy. A keyword-overlap router scores incoming messages against skill summaries and binds the best match for the session's lifetime; explicit binding (`create_session_with_skill`) bypasses routing entirely for agents with a single fixed purpose.

Under the hood: a three-layer memory architecture (in-process cache → durable SQLite transcript → distilled long-term knowledge), pluggable reasoning strategies (ReAct, Plan, Hierarchical) wired as pure `Vec<Message> → String` functions with no memory coupling, and MCP support on both the client and server side. The optional HTTP sidecar is agent-owned — spawned by the agent, not the other way around.

## Architecture

```
your binary
  ├── agentverse_agent::Agent
  │     ├── agentverse_strategy::build() → Arc<dyn RunStrategy>
  │     │     ├── ReActStrategy  (avs-react)
  │     │     ├── PlanStrategy   (avs-plan)
  │     │     └── HierarchicalStrategy (avs-plan)
  │     ├── LlmRunner  (avs-core)
  │     ├── ToolRegistry  (avs-tools)
  │     ├── SkillConfig  (avs-skill, optional)
  │     │     ├── SkillRegistry — loaded from skills/system/ and skills/user/
  │     │     ├── SkillMode — Open (any skill) or Constrained (allowlist)
  │     │     └── SkillRouter — keyword-overlap routing on first invoke
  │     ├── Memory layers
  │     │     ├── Layer 1: CacheMemory   — in-process RAM buffer, TTL-evicted
  │     │     ├── Layer 2: SessionMemory — durable per-user conversation transcript (SQLite/Postgres)
  │     │     └── Layer 3: LongtermMemory — distilled cross-session knowledge (vector store, optional)
  │     └── HTTP server (avs-agent `http` feature, optional)
  └── agentverse_subagent::SubAgentExecutor (optional, for multi-agent pipelines)
        ├── run(&spec, ctx)              — single subagent, sequential
        ├── run_many(tasks)              — parallel, results in completion order
        └── spawn(spec, ctx) → Handle   — parallel, input order via await_result()
```

`Agent` is the only way to invoke the LLM. You choose a strategy with `agentverse_strategy::build(StrategyKind::*)` and pass it to `Agent::new`. The agent handles session history, memory assembly, prompt construction, skill routing, and optional HTTP serving.

## What Is Implemented

- **Agent**: `agentverse-agent::Agent` is the single LLM access point. Composes `LlmRunner`, `StrategyKind`, `SessionManager`, memory layers, and optional skill routing.
- **Strategies**: ReAct, Plan-and-Execute, Hierarchical planning. Selected at construction via `agentverse-strategy::build`. Strategies are pure `Vec<Message> → String` with no memory coupling.
- **Skill system**: `agentverse-skill` provides file-based skill discovery, keyword-overlap routing, and per-session skill context. Skills are Markdown files (`SKILL.md`) that declare LLM instructions, tool allowlists, and metadata. Two load slots — `system/` and `user/` — support operator overrides without code changes.
- **Three-layer memory**: Layer 1 `CacheMemory` (in-process, TTL), Layer 2 `SessionMemory` (durable transcript), Layer 3 `LongtermMemory` (distilled cross-session knowledge, opt-in).
- **Subagent runtime**: `agentverse-subagent` provides isolated, budget-limited worker agents (`SubAgentExecutor`). Each subagent runs its own ReAct loop with a scoped tool registry, a step/token/timeout budget, and returns a single text answer. Supports programmatic orchestration (`run`, `run_many`, `spawn`) and LLM-driven dispatch via the `spawn_subagent` tool.
- **Multi-user sessions**: `Agent` routes through `SessionManager` for durable per-user conversation history with ownership enforcement.
- **HTTP sidecar**: `Agent::new(..., enable_http_server: true)` spawns an HTTP server as a background task. The agent can run without it; the server cannot run without the agent.
- **Agent-owned integrations**: `IntegrationRuntime` reads connector config, starts Slack/GitHub/WhatsApp or console connectors, calls an agent handler, and sends responses.
- **Strategies and tools**: `ToolRegistry`, built-in tools, and MCP adapters are wired into strategies at agent construction.

## Quick Start

### Prerequisites

- Rust 1.75+
- An OpenAI-compatible model endpoint, Anthropic API key, or Gemini API key

### Run an Example Agent

```bash
# Local OpenAI-compatible endpoint (no API key required)
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=your-model \
cargo run -p example-hello-agent

# Hosted OpenAI endpoint
MODEL_BASE_URL=https://api.openai.com/v1 \
MODEL_API_KEY=sk-xxx \
MODEL_NAME=gpt-4o \
cargo run -p example-hello-agent
```

Other OpenAI-compatible endpoints work by changing `MODEL_BASE_URL`: Ollama, llama.cpp, vLLM, LM Studio, Groq, Together AI.

### Run an HTTP Agent

`example-http-agent` builds an `Agent` with `enable_http_server=true` and keeps the process alive:

```bash
ANTHROPIC_API_KEY=sk-ant-... \
MODEL_NAME=claude-sonnet-4-6 \
HOST=0.0.0.0 PORT=3000 \
cargo run -p example-http-agent
```

The agent listens on `0.0.0.0:3000` by default.

```bash
curl http://localhost:3000/health
curl http://localhost:3000/ready
```

## HTTP API

### `GET /health`

```json
{"status":"healthy","model":"claude-sonnet-4-6"}
```

### `GET /ready`

```json
{"status":"ready"}
```

### `POST /invoke`

Stateless single-message invocation. Does not persist conversation history.

```bash
curl -X POST http://localhost:3000/invoke \
  -H "Content-Type: application/json" \
  -d '{"user_id":"user1","message":"Hello, agent!"}'
```

Response:

```json
{"message":"Hello! How can I help you?","user_id":"user1"}
```

### Session Routes

Sessions persist per-user conversation history in SQLite by default.

```bash
# Create a session
curl -X POST http://localhost:3000/sessions \
  -H "Content-Type: application/json" \
  -d '{"user_id":"alice"}'
# → {"session_id":"<uuid>"}

# Send a message
curl -X POST http://localhost:3000/sessions/<session_id>/messages \
  -H "Content-Type: application/json" \
  -d '{"user_id":"alice","message":"Remember that my favorite color is green."}'
# → {"session_id":"<uuid>","reply":"..."}

# Get session metadata
curl 'http://localhost:3000/sessions/<session_id>?user_id=alice'

# End a session
curl -X DELETE http://localhost:3000/sessions/<session_id> \
  -H "Content-Type: application/json" \
  -d '{"user_id":"alice"}'
```

Session ownership is enforced: `user_id` is checked against the session before any access.

### `POST /aether/invoke`

Aether-compatible HTTP invoke route.

```bash
curl -X POST http://localhost:3000/aether/invoke \
  -H "Content-Type: application/json" \
  -d '{"id":"<uuid>","kind":"invoke","payload":{"input":"Hello"},"metadata":{}}'
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|---|---:|---|
| `MODEL_BASE_URL` | `http://localhost:9090/v1` | OpenAI-compatible base URL |
| `MODEL_API_KEY` | empty | Provider API key (optional when `MODEL_BASE_URL` is set for local endpoints) |
| `MODEL_NAME` | `gpt-4` | Model name |
| `ANTHROPIC_API_KEY` | — | Anthropic API key (for Anthropic provider) |
| `HOST` | `0.0.0.0` | HTTP server bind address |
| `PORT` | `3000` | HTTP server port |
| `API_KEY` | unset | Optional bearer token required by all HTTP routes when set |
| `RUST_LOG` | `info` | Tracing level filter |
| `LOG_FORMAT` | text | Set to `json` for JSON logs |

### YAML Config

```yaml
agent:
  provider:
    type: openai
    model_name: "gpt-4o"
    api_key: "sk-xxx"
    base_url: "https://api.openai.com/v1"
  max_iterations: 10
```

## Multi-User Sessions

`agentverse-agent` owns the top-level `Agent`. `agentverse-session` provides session data infrastructure only.

```text
Agent::invoke(user_id, session_id, input)
  1. assert_owner(user_id, session_id)
  2. get/rehydrate Layer-1 CacheMemory from Layer-2 SessionMemory if cold
  3. retrieve scored Layer-3 LongtermMemory for the input (optional)
  4. assemble messages: [system + long-term context] + cache + user_input
  5. strategy.run(messages)
  6. append_turn to Layer-1 cache and Layer-2 SessionMemory
  7. async: consolidate turn into Layer-3 LongtermMemory
  8. return assistant text
```

Available session memory backends:

- `SqliteSessionMemory` in `agentverse-session` (default)
- `PostgresSessionMemory` in `agentverse-memory-pgvector`

## Integrations

`agentverse-integration` is owned by the agent. The agent creates an `IntegrationRuntime` and provides a handler; the runtime handles connector I/O.

```rust
use agentverse_integration::{Event, IntegrationRuntime};
use std::sync::Arc;

let agent = Arc::new(Agent::new(/* ... */));
let runtime = IntegrationRuntime::from_config("agent.toml").await?;

runtime
    .run(move |event: Event| {
        let agent = Arc::clone(&agent);
        async move {
            let answer = agent.invoke_stateless(&event.text).await
                .map_err(|e| agentverse::AgentError::Memory(e.to_string()))?;
            Ok::<Event, agentverse::AgentError>(Event { text: answer, ..event })
        }
    })
    .await?;
```

If `agent.toml` is missing, `IntegrationRuntime::from_config` falls back to `ConsoleConnector`.

Supported connectors: Console, Slack, GitHub, WhatsApp.

## Strategies and Tools

Strategies are selected via `agentverse-strategy::build`:

```rust
use agentverse_strategy::{build, StrategyKind};

let strategy = build(StrategyKind::React, runner, prompts, tools, 10);
// StrategyKind::Plan
// StrategyKind::Hierarchical
```

Underlying strategy crates:

- `agentverse-react`: ReAct loop (supports parallel tool dispatch via `ToolCalls`)
- `agentverse-plan`: Plan-and-Execute and hierarchical planning
- `agentverse-router`: dynamic strategy routing

Built-in tools in `agentverse-tools`:

- `Calculator`, `DateTimeTool`, `FileSearch`, `HttpClient`, `ShellTool`, `WebSearch`
- `FindToolsTool` — auto-registered meta-tool; BM25 keyword search over the registry
- `ActiveToolSet` — per-invocation filter controlling which tool schemas appear in the LLM prompt

Tools implement the `Tool` trait with a strongly-typed `Args` struct (schema derived automatically via `schemars`). The registry stores them as `Arc<dyn ErasedTool>` for object-safe dispatch.

## MCP (Model Context Protocol)

`agentverse-mcp` supports both sides of the MCP protocol:

**Client** — discover and use tools from any MCP server:

```rust
use agentverse_mcp::{McpCatalogSource, McpClient, McpTransport};

let transport = McpTransport::StreamableHttp {
    endpoint: "https://tools.example.com/mcp".parse().unwrap(),
    headers: Default::default(),
};
let client = McpClient::connect(transport).await?;
McpCatalogSource::populate(&registry, &client).await?;
// registry now contains all tools from the remote server
```

**Server** — expose a `ToolRegistry` over HTTP as an MCP endpoint:

```rust
use agentverse_mcp::McpServer;

let mut server = McpServer::new(Arc::clone(&registry));
let port = server.bind_random_port().await?;
tokio::spawn(async move { server.run().await });
```

**TOML config** — load multiple MCP servers at startup via `McpLoader`:

```toml
# agent.toml (excerpt)
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
```

```rust
use agentverse_mcp::{McpLoader, McpServerConfig};

let configs: Vec<McpServerConfig> = toml::from_str(&config_str)?;
McpLoader::load(&registry, &configs).await?;
```

Stdio transport spawns a subprocess; Streamable HTTP uses the MCP 2025-03-26 spec.

## Subagents

`agentverse-subagent` lets you orchestrate isolated, budget-limited worker agents from your binary or from within an agent's skill. Each subagent runs its own ReAct loop with a scoped tool set and returns a single text answer — invisible to the parent's session history.

**Programmatic orchestration** — `SubAgentExecutor` drives subagents directly from Rust:

```rust
use agentverse_subagent::{
    Budget, ResourceContent, SubAgentContext, SubAgentExecutor, SubAgentHandle, SubAgentSpec,
};
use std::sync::Arc;
use std::time::Duration;

let executor = SubAgentExecutor::new(
    Arc::clone(&connection_manager),
    Arc::clone(&tools),   // subagents only see tools named in spec.allowed_tools
    Arc::clone(&prompts),
);

// Run one subagent sequentially
let result = executor.run(&SubAgentSpec {
    name: "analyst".into(),
    objective: "Estimate the NPV for project X assuming 12% discount rate.".into(),
    system_prompt: Some("You are a financial analyst.".into()),
    model: None,   // inherit parent model; or Some(ModelOverride::Alias("haiku"))
    allowed_tools: vec!["npv_calculator".into()],
    budget: Budget { max_steps: 8, max_tokens: 4000, timeout: Duration::from_secs(90) },
}, SubAgentContext { resources: vec![], depth: 0 }).await?;

// Run multiple subagents in parallel — spawn+await_result preserves input order
let labeled: Vec<(&str, SubAgentHandle)> = vec![
    ("Financial", executor.spawn(financial_spec, ctx.clone())),
    ("Timeline",  executor.spawn(timeline_spec,  ctx.clone())),
    ("Risk",      executor.spawn(risk_spec,       ctx.clone())),
];
for (label, handle) in labeled {
    println!("{}: {}", label, handle.await_result().await?.answer);
}

// Chain outputs via ResourceContent
let synthesis_ctx = SubAgentContext {
    resources: vec![
        ResourceContent { label: "Financial".into(), content: financial.answer },
        ResourceContent { label: "Risk".into(),      content: risk.answer },
    ],
    depth: 0,
};
```

**LLM-driven orchestration** — register `SubAgentTool` so the LLM can call `spawn_subagent` as a tool. A `SKILL.md` body instructs the model when and how to delegate:

```rust
let executor = Arc::new(SubAgentExecutor::new(cm, tools, prompts));
let agent_tools = ToolRegistry::new();
SubAgentExecutor::register_tool(&executor, &agent_tools);
// SKILL.md body tells the LLM to call spawn_subagent for specific workflows
```

Depth is hard-limited to 1 — subagents cannot spawn nested subagents.

See `examples/project-feasibility` (programmatic) and `examples/business-report` (LLM-driven).

## Prompt Templates

AgentVerse uses `PromptRegistry` to load embedded defaults and optional `.j2` templates from a prompts directory.

Common layout:

```text
prompts/
  system.j2
  react.j2
  react_examples.toml
```

Example:

```rust
use agentverse::{PromptConfig, PromptRegistry};

let registry = Arc::new(PromptRegistry::from_config(&PromptConfig {
    prompts_dir: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string()),
    ..Default::default()
})?);
```

## Crates

| Crate | Package | Purpose |
|---|---|---|
| `avs-core` | `agentverse` | Core config, errors, prompts, model providers, `LlmRunner`, memory and tool traits |
| `avs-agent` | `agentverse-agent` | Single LLM access point: `Agent` composes `LlmRunner`, strategy, `SessionManager`, and optional `SkillConfig`; optional HTTP sidecar |
| `avs-skill` | `agentverse-skill` | Skill system: `SKILL.md` parser, `SkillRegistry`, `SkillRouter`, `SkillMode`, `SkillConfig` |
| `avs-strategy` | `agentverse-strategy` | Strategy factory (`build`, `StrategyKind`) and umbrella re-exports |
| `avs-session` | `agentverse-session` | Session model, `SessionManager`, `SessionMemory` trait, `SqliteSessionMemory` |
| `avs-integration` | `agentverse-integration` | Agent-owned connector runtime for console, Slack, GitHub, WhatsApp |
| `avs-react` | `agentverse-react` | ReAct strategy loop |
| `avs-plan` | `agentverse-plan` | Plan-and-Execute and hierarchical strategies |
| `avs-router` | `agentverse-router` | Strategy router |
| `avs-tools` | `agentverse-tools` | Built-in tools, `ToolRegistry` with BM25 search, `ActiveToolSet`, parallel dispatch |
| `avs-mcp` | `agentverse-mcp` | MCP client (stdio + Streamable HTTP), `McpServer`, `McpCatalogSource`, `McpLoader` |
| `avs-subagent` | `agentverse-subagent` | Subagent runtime: `SubAgentExecutor`, `SubAgentSpec`, `Budget`, `SubAgentHandle`, `SubAgentTool` (`spawn_subagent`) |
| `avs-memory` | `agentverse-memory` | Layer-1 working buffer (`SimpleMemory`, `AgentMemory`) and `LongTermBackend` trait |
| `avs-memory-lancedb` | `agentverse-memory-lancedb` | LanceDB long-term memory backend |
| `avs-memory-pgvector` | `agentverse-memory-pgvector` | pgvector memory backend and Postgres session store |
| `avs-guardrails` | `agentverse-guardrails` | Prompt, output, action, and rate-limit guardrails |
| `avs-logging` | `agentverse-logging` | Tracing subscriber initialization |

## Examples

The three skill examples form a progression of skill-system concepts:

| Package | Skill mode | Binding | Concept demonstrated |
|---|---|---|---|
| `example-hello-agent` | `Open` | Auto-routing | General-purpose REPL; Extend pattern (user/ adds travel-advisor) |
| `example-web-search-agent` | `Constrained(["web-search"])` | Auto-routing | Constrained routing; Shadow pattern (user/ overrides system web-search) |
| `example-code-review-agent` | `Open` | Explicit (`create_session_with_skill`) | Explicit binding; tool restriction (file_search + shell only) |

Multi-agent examples (require a local LLM — set `MODEL_BASE_URL`):

| Package | Orchestration | Concept demonstrated |
|---|---|---|
| `agentverse-demo-tools` | — (library) | Six domain tools (`ProjectCostEstimator`, `NpvCalculator`, `MilestoneScheduler`, `RunwayProjector`, `MarketSizingCalculator`, `RiskAdjustedSchedule`) exposed via MCP; shared by both examples below |
| `example-project-feasibility` | Programmatic | `SubAgentExecutor::spawn` fans out three analyst subagents in parallel; a synthesis subagent reads all three as `ResourceContent` |
| `example-business-report` | LLM-driven | `SubAgentTool` registered in an `Agent`; `business-report` skill instructs the LLM to spawn three analyst subagents and synthesize a report |

Other examples:

| Package | Description |
|---|---|
| `example-react-calculator` | ReAct calculator demonstration |
| `example-anthropic-react` | Anthropic ReAct agent with prompt-cache usage |
| `example-slack-hr-assistant` | IntegrationRuntime-backed Slack/console assistant |
| `example-http-agent` | Agent with `enable_http_server=true`; demonstrates the HTTP sidecar |
| `example-mcp-demo` | Full MCP round-trip: `McpServer` exposes tools; `McpCatalogSource` discovers them into a second registry; agent uses them transparently |

Run examples with `cargo run -p <package>`.

## Skills

Skills are Markdown files that give an agent focused instructions, a tool allowlist, and metadata — without any code change. Each skill is a directory containing a `SKILL.md` file:

```
skills/
  system/
    math-helper/SKILL.md      # ships with the agent
    datetime-helper/SKILL.md
  user/
    travel-advisor/SKILL.md   # operator-added (Extend pattern)
```

**`SKILL.md` format:**

```markdown
---
name: math-helper
description: >
  Performs arithmetic and unit conversions.
  Use when the user asks to calculate, compute, add, subtract,
  multiply, or divide numbers.
version: 1.0.0
agentverse:
  tools:
    - calculator
---

You are a precise math assistant. Use the calculator tool for all
arithmetic — never compute in your head. Show your working steps clearly.
```

The `agentverse.tools` list restricts which registered tools the LLM can call in that session. Tools not listed are invisible to the LLM for that session even if registered on the agent.

**Wiring into `Agent::new`:**

```rust
use agentverse_agent::{Agent, SkillConfig, SkillMode};

let skills_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/skills");
let skills = SkillConfig::load(skills_dir, SkillMode::Open)
    .expect("skills dir not found");

let agent = Agent::new(runner, tools, prompts, session_memory, strategy,
                       false, None, Some(skills));
```

**Routing modes:**

| Mode | Behavior |
|---|---|
| `SkillMode::Open` | All registered skills are candidates; router scores by keyword overlap against description |
| `SkillMode::Constrained(ids)` | Only the listed skill IDs are candidates; lower routing threshold (0.08 vs 0.15) |

On first `invoke`, the `SkillRouter` scores the user message against eligible skills. The top match above threshold binds the skill to the session for its lifetime. All subsequent messages in that session use the bound skill's instructions and tool allowlist.

**Explicit binding** (bypasses routing entirely):

```rust
let session_id = agent
    .create_session_with_skill("user", "code-review")
    .await?;
```

**Shadow and Extend patterns:**

- **Shadow** — a `user/` skill with the same `name:` as a `system/` skill replaces it. Operators can swap instructions without touching code.
- **Extend** — a `user/` skill with a new name adds capability. If it declares `tools: []`, it runs as pure language generation.

**Hot reload:**

```rust
agent.reload_skills().await?;
```

Existing sessions are unaffected; new routing calls pick up the refreshed registry.

## Development

```bash
cargo fmt --all --check
cargo clippy --all -- -D warnings
cargo test --workspace
```

See [DEVELOPMENT.md](DEVELOPMENT.md) for deeper implementation notes.

## Project Structure

```text
AgentVerse/
|-- avs-core/
|-- avs-agent/
|-- avs-skill/
|-- avs-session/
|-- avs-strategy/
|-- avs-integration/
|-- avs-react/
|-- avs-plan/
|-- avs-router/
|-- avs-tools/
|-- avs-mcp/
|-- avs-subagent/
|-- avs-memory/
|-- avs-memory-lancedb/
|-- avs-memory-pgvector/
|-- avs-guardrails/
|-- avs-logging/
`-- examples/
    |-- demo-tools/          (library: 6 MCP-exposed domain tools)
    |-- project-feasibility/ (programmatic multi-agent pipeline)
    `-- business-report/     (LLM-driven multi-agent via skill)
```

## License

MIT
