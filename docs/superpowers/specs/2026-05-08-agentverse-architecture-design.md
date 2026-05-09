# AgentVerse Architecture Design

> **Status:** Approved  
> **Date:** 2026-05-08  
> **Topic:** AgentVerse Rust AI Agent Framework

---

## 1. Project Overview

AgentVerse is a lightweight, high-performance, extensible enterprise AI Agent framework written in Rust. It supports scenarios ranging from simple scripts to complex internal enterprise assistants — IT service desks, HR assistants, internal knowledge Q&A, multi-system automation — while prioritizing production readiness, security, and developer experience.

**Key principles:**
- Lightweight: minimal core dependencies, small binary, fast compile and startup
- Highly extensible: all critical capabilities are pluggable (Tools, Strategies, Integrations, Prompts)
- Unified abstraction: consistent, simple public API
- Enterprise-ready: built-in Tracing, Guardrails, RBAC, audit logging
- Model-agnostic: supports OpenAI, Anthropic, Ollama, Groq, vLLM, etc.
- Security-first: WASM sandbox (future), permission control, prompt injection protection

**Not in scope:** Multi-agent orchestration. A separate project will handle orchestrating multiple AgentVerse instances.

---

## 2. Crate Structure (Cargo Workspace)

AgentVerse uses a Cargo workspace with multiple crates under the `avs-*` prefix. All crates share a unified version number.

| Crate | Responsibility | Key Dependencies |
|---|---|---|
| `avs-core` | Agent core: Builder, Config, Runtime, Tool trait, ModelProvider, AgentError, PromptRegistry | serde, thiserror, async-trait, tracing |
| `avs-react` | ReAct orchestration strategy | avs-core, minijinja |
| `avs-plan` | Plan-and-Execute + Hierarchical Planning strategies | avs-core |
| `avs-router` | Strategy Router (LLM-based dynamic routing) | avs-core |
| `avs-memory` | Layered memory system (Short-term + Long-term) | serde, avs-core |
| `avs-memory-lancedb` | LanceDB backend for long-term memory | lancedb, avs-memory |
| `avs-memory-pgvector` | pgvector backend for long-term memory | sqlx, avs-memory |
| `avs-tools` | Built-in tools (file search, HTTP client, calculator, etc.) | avs-core, reqwest |
| `avs-mcp` | MCP client + `with_mcp()` injection into ToolRegistry | avs-core, reqwest |
| `avs-guardrails` | Prompt/Output/Action filtering, rate limiting, cost control | avs-core |
| `avs-integration` | IntegrationAdapter trait + Slack + Webhook adapters | avs-core, axum |
| `avs-server` | Standalone HTTP server (Axum-based) | avs-core, axum, tracing |

---

## 3. Core Architecture

### 3.1 Agent Lifecycle

```
User Input → Integration Adapter → Strategy Router (optional) → Strategy Loop → Output
                                                     ↓
                            [Prompt] → [LLM] → [Tool Execute] → [Memory Update] → Loop
```

### 3.2 Agent Builder & Configuration

Two complementary ways to create an Agent:

**Code configuration (Builder pattern):**
```rust
let agent = Agent::builder()
    .strategy(ReAct::new())
    .model(OpenAICompatible::new("gpt-4", api_key))
    .tool(FileSearch)
    .guardrails(Guardrails::default())
    .build();
```

**Configuration-driven (for DSL / file loading):**
```rust
let config = Config::from_file("agent.yaml")?;
let agent = Agent::from_config(config)?;
```

**Key design: `Config` is a first-class citizen, not a Builder byproduct.**
- `Config` is a pure data structure (`#[derive(Serialize, Deserialize)]`), mappable to YAML/JSON
- Builder is the "user-friendly API" for Config
- DSL compiles to Config, then builds Agent
- Config validation happens at the Config layer

`Config` and `Agent` can be converted bidirectionally:
```rust
let config: Config = agent.into();
let agent = Agent::from_config(config);
```

### 3.3 Concurrency & State Management

- **Single-instance, high-concurrency model**: one Agent instance serves all users via `Arc<RwLock<AgentState>>`
- State is partitioned by `user_id` in `ShortTermMemory` (`HashMap<user_id, Vec<Message>>`)
- Mixed concurrency: async for network IO (LLM calls), `spawn_blocking` for CPU-intensive sync tools
- Tokio multi-thread runtime for true parallelism

### 3.4 Error Handling

Layered error types:
```rust
enum AgentError {
    Model(ModelError),
    Tool(ToolError),
    Config(ConfigError),
    Guardrail(GuardrailError),
    Memory(MemoryError),
    // ...
}

enum ModelError {
    ApiError(String),
    Timeout(String),
    InvalidResponse(String),
}
```

Users can `match` for precise handling or use `?` to propagate.

### 3.5 LLM Client

- **Library**: `reqwest` with connection pooling
- **Supported providers**: OpenAI-compatible API (covers OpenAI, Ollama, vLLM, Groq, etc.) + Anthropic
- **Reliability**: automatic retry with exponential backoff + circuit breaker
- Model provider abstraction via `ModelProvider` trait

### 3.6 Orchestration Strategy

**Pattern**: Fixed cycle skeleton with phase-aware context.

```rust
enum StepContext {
    Planning { available_tools: Vec<Tool> },
    Executing { plan: &[Step], memory: &Memory },
    Thinking { conversation: &[Message] },
    Acting { tool: &Tool, context: &ToolContext },
}

enum StepOutcome {
    Think { next_action: Action },
    Act { tool_name: String, result: ToolResult },
    Plan { plan: Vec<Step> },
    Execute { step_index: usize, result: ToolResult },
    Done { output: String },
    Error { message: String },
}
```

The cycle skeleton (shared across strategies) handles: loop control, prompt generation, LLM invocation, tool execution, memory update. Each strategy only implements `step()` to decide what happens next within its phase.

**Strategies:**
- **ReAct**: think → act → observe → repeat
- **Plan-and-Execute**: generate plan → execute steps sequentially
- **Hierarchical Planning**: decompose → generate detailed plan → execute steps (NOT multi-agent; single agent with complex planning)
- **Strategy Router** (optional): LLM-based dynamic routing between strategies at runtime

### 3.7 Tool Abstraction

**Two trait variants:**
- `SyncTool` — for CPU-bound or blocking operations
- `AsyncTool` — for network-bound operations

**Built-in tools** use Rust struct types for compile-time parameter validation:
```rust
struct FileSearchArgs {
    path: String,
    pattern: String,
}
```

**MCP tools** use `serde_json::Value` for runtime validation (dynamic schema).

**ToolRegistry** supports static registration + runtime dynamic registration (hot-plug). MCP tools are injected via `McpToolRegistry::with_mcp()`.

### 3.8 Prompt Management

- **Engine**: minijinja
- **Default templates**: compiled at build time via `include_str!`
- **Custom templates**: loaded at runtime, override defaults by name
- Managed by `PromptRegistry`
- Supports: system prompts, strategy-specific prompts, router prompts, few-shot examples

### 3.9 Memory System

**Layered design:**

**Short-term Memory** (Conversation Buffer):
- In-memory `Vec<Message>` partitioned by `user_id`
- Supports auto-summary for long conversations

**Long-term Memory** (Vector Database):
- Pluggable backends via `LongTermMemory` trait
- MVP backends:
  - **LanceDB** (embedded, zero-config, local dev)
  - **pgvector** (PostgreSQL extension, enterprise deployment)
- Migration path: dev (LanceDB) → prod (pgvector) via config change

### 3.10 Guardrails

**Default-integrated security layer** (not opt-in):

- `PromptGuard` — detect prompt injection, jailbreak attempts
- `OutputGuard` — filter/validate LLM output (PII detection, sensitive word filtering)
- `ActionGuard` — dangerous action confirmation (file write, command exec, delete → human-in-the-loop)
- `RateLimiter` — request throttling, cost control

### 3.11 Human-in-the-Loop

**Message queue mode** (async confirmation):
- `ActionGuard` triggers → suspends strategy loop → pushes `Action` to queue → waits for external confirmation
- Supports async confirmation (e.g., Slack reply "approve")
- Does NOT block the entire loop with synchronous callbacks

### 3.12 Integration Adapters

**Framework-provided (MVP):**
- Slack (WebSocket/Bolt mode)
- REST API / Webhook (generic HTTP endpoint)

**User/community-provided:**
- Teams
- Enterprise WeChat (企业微信)
- Discord
- Custom protocols

Base trait: `IntegrationAdapter` (in `avs-integration`).

### 3.13 Tracing & Observability

- **Logging**: `tracing` + `tracing-subscriber` (JSON for production, colored text for dev)
- **Tracing**:
  - Base `NoopTracer` in core — zero overhead when disabled
  - OpenTelemetry adapter via optional `tracing` feature flag
  - Users disable tracing: `default-features = false`
- Full end-to-end trace lifecycle: input → strategy → LLM → tool → output

### 3.14 Authentication

- **API Key** authentication via `Authorization: Bearer <key>` header
- Key binds to `user_id` for multi-tenant support
- OAuth 2.0 / OIDC planned for v0.3+

### 3.15 Agent Public Interface

For future multi-agent orchestration frameworks:
- `AgentId`, `AgentMetadata`, `AgentInput`, `AgentOutput`, `AgentContext` core data structures
- `invoke()` and `invoke_with_context()` methods
- `health_check()` and basic status queries
- Message interface

---

## 4. Examples (5 shipped examples)

| Example | Purpose |
|---|---|
| `hello-agent` | Simplest Agent: ReAct + built-in tools |
| `slack-hr-assistant` | Slack integration + HR tools |
| `rag-qa` | Vector DB + knowledge base Q&A |
| `web-search-agent` | Search tool + Plan-and-Execute |
| `code-review-agent` | Code analysis + Hierarchical Planning |

Examples serve as "living documentation" and integration tests.

---

## 5. Development & CI

- **CI**: `cargo check` + `cargo test` + `cargo clippy` + `cargo fmt --check` + `cargo doc` + `cargo audit` + build all examples
- **Testing**: `mockall` for trait mocking, `httpmock` for HTTP (LLM API) mocking, custom `TestLLM` for strategy E2E
- **Git workflow**: main branch development, PR-based review
- **Versioning**: unified version across all crates in workspace

---

## 6. Roadmap Alignment

| Phase | Scope |
|---|---|
| v0.1 | Core + ReAct + Plan + Router + Memory (LanceDB+pgvector) + Tools + MCP + Prompt + Guardrails + Integration (Slack+Webhook) + Server + Tracing + 5 examples |
| v0.3+ | Graph Strategy, more built-in tools, Teams/Enterprise WeChat, OAuth2/OIDC, Python/TS bindings (PyO3 + wasm) |
| v0.5+ | VS Code / Web IDE support, Agent marketplace, distributed Agent cluster, WASM sandbox |

---

## 7. Decisions Log

| # | Decision | Choice |
|---|---|---|
| 1 | User positioning | D — Framework + 5 pre-configured examples |
| 2 | Model support | OpenAI-compatible + Anthropic |
| 3 | Runtime | Library + standalone server (Axum) |
| 4 | Crate structure | Workspace with multiple crates (`avs-*` prefix) |
| 5 | MCP relationship | MCP crate provides `with_mcp()` injection method |
| 6 | Memory system | Layered: Short-term (Vec) + Long-term (LanceDB/pgvector) |
| 7 | Concurrency model | Mixed: async for IO, spawn_blocking for sync tools |
| 8 | Strategy cycle | Fixed skeleton + phase-aware context |
| 9 | Integration | Framework provides Slack + Webhook; rest user-defined |
| 10 | Prompt templates | minijinja: compile-time defaults + runtime custom |
| 11 | Core dependencies | Minimal: serde, thiserror, async-trait, tracing |
| 12 | Guardrails | Independent layer, default-integrated |
| 13 | Tracing | Core zero-dependency base + OTel optional feature |
| 14 | Agent construction | Builder + Config hybrid |
| 15 | Error handling | Layered error types |
| 16 | State management | Single instance + Arc<RwLock> + user_id sharding |
| 17 | Tool parameters | Built-in static types + MCP dynamic Value |
| 18 | LLM client | reqwest + connection pooling |
| 19 | LLM reliability | Auto retry + circuit breaker |
| 20 | Human-in-the-loop | Message queue mode (async confirmation) |
| 21 | Authentication | API Key |
| 22 | Logging | tracing + tracing-subscriber |
| 23 | Testing | mockall + httpmock + TestLLM |
| 24 | CI scope | check + test + clippy + fmt + doc + audit + example builds |
| 25 | Version management | Unified version across workspace |
| 26 | Git workflow | main branch, PR-based |
