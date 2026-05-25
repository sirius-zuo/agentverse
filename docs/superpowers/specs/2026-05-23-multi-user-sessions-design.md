# Multi-User Session Support Design

**Date:** 2026-05-23
**Scope:** Session management only — HITL (human-in-the-loop) is a separate future design.

---

## Goals

- A single `Agent` instance serves multiple users simultaneously.
- Each user can have multiple independent sessions.
- Each session has its own isolated conversation history and prompt cache lineage.
- Sessions survive process restarts (durable storage).
- Dev/QA runs fully embedded (no external services); production uses PostgreSQL.

---

## Component Overview

```
Entry points (Integration / REST API / CLI / direct call)
    │
    ▼
Agent                          ← top-level orchestrator
├── SessionManager             ← session lifecycle + message persistence
│   └── Arc<dyn SessionStore>  ← SQLite (dev) or PostgreSQL (prod)
└── LlmRunner                  ← stateless LLM invocation (current Agent, renamed)
    ├── PromptRegistry
    ├── Tracer
    └── ConnectionManager      ← HTTP connection + resilience
        ├── reqwest::Client
        ├── Box<dyn ModelProvider>   ← pure protocol translator
        ├── CircuitBreaker
        └── retry policy
```

---

## Key Decisions

### No connection pool for session isolation

Anthropic's prompt cache is isolated per workspace (as of Feb 2026); OpenAI, Gemini, and DeepSeek behave similarly. Cache entries are content-addressed within a workspace — two sessions with different conversation histories naturally get different cache entries regardless of which HTTP client sent the request. A per-session connection pool adds no isolation benefit for current providers.

Session isolation is enforced entirely at the **data layer**: each session owns its own `messages` history. `ConnectionManager` is shared across all sessions via `LlmRunner`.

A future stateful provider (e.g., one with true server-side session threads) can extend this design without invalidating it.

### Verbatim message storage

For cache hit fidelity on resume, stored messages must replay byte-for-byte identically to what was previously sent. No transformation, summarization, or re-encoding at the storage layer. The full conversation history is the source of truth.

For very long conversations, the semantic memory backends (`LanceDB` / `pgvector`) provide a selection layer via `prime_from_long_term()` — pulling the most relevant past context rather than replaying all turns. This is complementary, not a replacement.

### Multiple cache breakpoints

Anthropic supports up to 4 `cache_control` breakpoints per request. For long resumed sessions, breakpoints placed at regular intervals (e.g., every 50 turns) allow partial cache hits even when only the tail of the conversation has changed. Breakpoint placement is handled at the wire-request building layer, not at storage.

---

## `avs-core` Refactoring

### `ModelProvider` — pure protocol translator

**Before:** `AnthropicProvider`, `OpenAICompatible`, `GeminiProvider` each own a `reqwest::Client`, `api_base`, `api_key`, and `model_name`.

**After:** `ModelProvider` is a stateless protocol translator. It knows how to format a `GenerateRequest` into the provider's wire format and parse the response. It owns no HTTP client and no connection configuration.

```
trait ModelProvider: Send + Sync {
    fn name(&self) -> &str;
    // Returns the provider-specific JSON request body and endpoint path
    fn build_request(&self, model: &str, request: GenerateRequest) -> Result<(serde_json::Value, &str)>;
    fn parse_response(&self, body: &str) -> Result<GenerateResponse>;
    fn auth_headers(&self, api_key: &str) -> HeaderMap;
}
```

`AnthropicProvider`, `OpenAICompatible`, and `GeminiProvider` lose all fields — they become pure stateless formatters.

### `ConnectionManager` — renamed and restructured from `ProviderWrapper`

`ConnectionManager` owns everything needed to make one HTTP call to an LLM provider:

- `client: reqwest::Client` — the HTTP connection
- `api_base: String`, `api_key: String`, `model_name: String` — connection config
- `provider: Box<dyn ModelProvider>` — protocol translator
- `circuit_breaker: CircuitBreaker` — connection health state
- `max_retries`, `retry_delay_ms` — retry policy

`ConnectionManager::generate(request)` orchestrates:
1. Check circuit breaker — reject immediately if open
2. Call `provider.build_request()` to produce wire-format body + endpoint
3. Send via `self.client` with auth headers
4. Call `provider.parse_response()` on the response body
5. On transient failure: retry with exponential backoff; record to circuit breaker
6. Log prompt, response, and token usage

### `LlmRunner` — renamed from current `Agent`

The current `Agent` struct is renamed to `LlmRunner`. It is stateless with respect to conversation history — it takes a full message list, invokes the LLM, and returns a response. It owns:

- `connection_manager: Arc<ConnectionManager>`
- `prompt_registry: PromptRegistry`
- `tracer: Box<dyn Tracer>`
- `config: Config`

```
pub async fn invoke(
    &self,
    messages: Vec<Message>,  // full conversation history, caller-owned
) -> Result<GenerateResponse, LlmError>
```

This renders the system prompt via `prompt_registry`, builds a `GenerateRequest`, and calls `connection_manager.generate()`. It does not read or write any session state.

### `Agent` — new top-level orchestrator

**Implementation note:** The top-level `Agent` lives in `avs-agent` (`agentverse-agent` package), not in `avs-session`. `avs-session` owns only session data types, `SessionManager`, `SessionStore`, and concrete session stores.

`Agent` is a new struct that composes `LlmRunner` and `SessionManager`. It is the primary entry point for all callers.

```
pub struct Agent {
    runner: Arc<LlmRunner>,
    sessions: Arc<SessionManager>,
}
```

```
pub async fn invoke(
    &self,
    session_id: SessionId,
    input: &str,
) -> Result<String, AgentError>
```

Orchestrates:
1. Load message history via `sessions.load_messages(session_id)`
2. Append the new user message
3. Call `runner.invoke(messages)`
4. Append the assistant response
5. Persist both messages via `sessions.append_message()`
6. Return response content

Additional methods:
- `create_session(user_id) -> SessionId`
- `end_session(session_id)`
- `list_sessions(user_id) -> Vec<Session>`

---

## New Crate: `avs-session`

### `Session`

```
Session {
    id: SessionId,          // UUID
    user_id: String,
    status: SessionStatus,  // Active | Completed
    created_at: DateTime,
    updated_at: DateTime,
}
```

Messages are stored in a separate normalized table. `Session` holds no message data in memory.

### `SessionStatus`

```
enum SessionStatus {
    Active,
    Completed,
}
```

HITL states (`AwaitingHuman`, etc.) are deferred to the HITL design.

### `SessionStore` trait

```
#[async_trait]
trait SessionStore: Send + Sync {
    async fn create(&self, user_id: &str) -> Result<Session>;
    async fn get(&self, session_id: SessionId) -> Result<Option<Session>>;
    async fn update_status(&self, session_id: SessionId, status: SessionStatus) -> Result<()>;
    async fn list_by_user(&self, user_id: &str) -> Result<Vec<Session>>;
    async fn append_message(&self, session_id: SessionId, message: Message) -> Result<()>;
    async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<()>;
    async fn load_messages(&self, session_id: SessionId) -> Result<Vec<Message>>;
}
```

Ownership checks are performed by the top-level `Agent` through `SessionManager::assert_owner(user_id, session_id)` before loading or mutating a session.

### `SessionManager`

```
struct SessionManager {
    store: Arc<dyn SessionStore>,
}
```

A pure data-access layer. Wraps `SessionStore` with no knowledge of LLM calls or `Agent`. Exposes session lifecycle and message persistence operations. Owned by `Agent`.

### `SqliteSessionStore` (dev/QA)

SQLite implementation of `SessionStore`. Schema:

```sql
CREATE TABLE sessions (
    id          TEXT    PRIMARY KEY,
    user_id     TEXT    NOT NULL,
    status      TEXT    NOT NULL DEFAULT 'active',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX idx_sessions_user ON sessions(user_id);

CREATE TABLE messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id   TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role         TEXT    NOT NULL,
    content      TEXT    NOT NULL,
    sequence_num INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    UNIQUE(session_id, sequence_num)
);

CREATE INDEX idx_messages_session ON messages(session_id, sequence_num);
```

Messages are appended one row at a time. Full history is loaded as `SELECT ... ORDER BY sequence_num`. Content is stored verbatim — no JSON blobs, no transformation.

### `PostgresSessionStore` (production)

Lives in `avs-memory-pgvector` alongside `PgVectorBackend` — they share the same PostgreSQL connection pool. Implements the same `SessionStore` trait with equivalent tables in PostgreSQL. Switching from `SqliteSessionStore` to `PostgresSessionStore` requires only a config change at startup.

> **Note:** `avs-memory-pgvector` now covers both vector memory and session storage. Consider renaming the crate to `avs-postgres` in a follow-up to reflect this broader responsibility.

---

## Entry Points

`Agent::invoke(session_id, input)` is a clean public API — any caller can use it directly. The integration layer (`avs-integration`) is one adapter that bridges external connectors to this API:

- **`avs-integration`** — wraps Slack, WhatsApp, GitHub, and Console connectors; maps incoming events to `Agent::invoke` calls. Console is a connector like any other.
- **REST API server** — can call `Agent::invoke` directly per HTTP request
- **CLI binary** — can call `Agent::invoke` directly
- **Embedded library** — application code calls `Agent::invoke` directly

No entry point is special. All paths converge on `Agent::invoke(session_id, input)`.

---

## Environment Strategy

| Component | Dev / QA | Production |
|-----------|----------|------------|
| Session + message storage | `SqliteSessionStore` | `PostgresSessionStore` (in `avs-memory-pgvector`) |
| Long-term semantic memory | `LanceDbBackend` | `PgVectorBackend` |
| External services required | None | PostgreSQL |

Switching is done at startup via config/env. `SessionManager` depends on `Arc<dyn SessionStore>` — it never sees the concrete type. No application code changes between environments.

---

## Data Flow: `Agent::invoke`

```
Entry point (Integration / REST API / CLI / direct)
    │
    ▼
Agent::invoke(session_id, input)
    │
    ├─ SessionManager::load_messages(session_id) ──► SQLite / PostgreSQL
    │
    ├─ append user message (in-memory)
    │
    ├─ LlmRunner::invoke(messages)
    │       │
    │       ├─ PromptRegistry::render("system", ctx)
    │       │
    │       └─ ConnectionManager::generate(request)
    │               │
    │               ├─ circuit breaker check
    │               ├─ ModelProvider::build_request()
    │               ├─ reqwest::Client POST
    │               ├─ ModelProvider::parse_response()
    │               └─ retry / circuit breaker record
    │
    ├─ append assistant message (in-memory)
    │
    ├─ SessionManager::append_message(user_msg) ──► SQLite / PostgreSQL
    ├─ SessionManager::append_message(asst_msg) ──► SQLite / PostgreSQL
    │
    └─ return response string
```

---

## Non-Goals (this design)

- HITL states (`AwaitingHuman`, `Resumed`) — separate design
- Session-level concurrency limiting / semaphore — can be added on top of `Agent` when needed
- Message summarization / compaction — deferred; semantic memory backends handle long-history selection
- Per-session tool sets or prompt overrides — future extension
