# HTTP Sidecar

## Purpose

`avs-agent`'s optional `http` feature is an axum-based HTTP surface that turns
a running `Agent` into a network service: stateless single-turn invocation,
per-user session CRUD and message history, health/readiness probes, and an
inbound `/aether/invoke` endpoint that lets an external Aether-compatible
caller trigger an invocation through a shared `Envelope` wire format. It
lives inside `avs-agent` rather than as a standalone crate or binary because
it has nothing to serve without an already-constructed `Agent` —
`AgentBuilder::with_http_server()` only sets a flag, and the axum listener is
spawned from inside `AgentBuilder::build()` once the `Agent` already exists as
an `Arc`, so the sidecar's runtime lifetime is strictly bounded by the
agent's. Both the compile-time (`#[cfg(feature = "http")]`) and runtime
(`with_http_server()` must be called explicitly) gates are opt-in: an `Agent`
built without either exposes no network surface at all.

## Position in the System

The `http` feature is not a separate crate — it is an optional compile-time
feature of `avs-agent` itself, declared in `avs-agent/Cargo.toml`
(`http = ["dep:axum", "dep:tower", "dep:tower-http",
"dep:agentverse-guardrails"]`) and gated behind `#[cfg(feature = "http")] mod
http;` in `avs-agent/src/lib.rs`. Enabling it adds exactly one workspace
dependency beyond what [Agent](agent.md) already needs:
[Guardrails](guardrails.md) (`agentverse-guardrails`, for `RateLimiter`) — per
`scripts/check-layering.sh`, `agentverse-agent` remains alone in Layer 4
regardless, since `avs-guardrails` is already a Layer 2 dependency of the
crate. The module also reaches into its own crate's `Agent`/`AgentError`
(`avs-agent/src/agent/mod.rs`) for every route handler, and into
[Session](session.md)'s `agentverse_session::SessionMemoryError` to translate
storage errors into HTTP status codes. Nothing in the workspace's library
graph consumes `avs-agent::http` — the module is declared `mod http;`, not
`pub mod http;`, so it has no visibility outside the crate at all. It is
reached only through `AgentBuilder::with_http_server()`
(`avs-agent/src/agent/builder.rs`), which in turn only `examples/http-agent`
calls.

## Architecture

```mermaid
classDiagram
    class HttpConfig {
        +host String
        +port u16
        +from_env() HttpConfig
    }
    class InvokeRequest {
        +user_id String
        +message String
    }
    class Envelope {
        +id Uuid
        +kind EnvelopeKind
        +payload Value
        +metadata HashMap~String, String~
    }
    class EnvelopeKind {
        <<enum>>
        Invoke
        Result
        Error
        Ping
        Pong
    }
    class CreateSessionRequest {
        +user_id String
    }
    class SendMessageRequest {
        +user_id String
        +message String
    }
    class ListMessagesQuery {
        +user_id String
        +limit Option~i64~
        +before Option~i64~
    }
    Envelope o-- EnvelopeKind
    Agent <.. InvokeRequest : invoke_stateless()
    Agent <.. Envelope : invoke_stateless() via aether_invoke
    Agent <.. SendMessageRequest : invoke()
    Agent <.. CreateSessionRequest : create_session()
    RateLimiter <.. InvokeRequest : checked first
    RateLimiter <.. SendMessageRequest : checked first
    RateLimiter <.. ListMessagesQuery : checked first
```

The module splits across seven files under `avs-agent/src/http/`. `mod.rs`
holds `build_router` (assembles the axum `Router`), `spawn_server` (the entry
point `AgentBuilder::build` calls), and the startup security guard
(`validate_bind_security`, `is_loopback_host`, `api_key_configured`).
`config.rs` defines `HttpConfig` (`host`/`port`, populated from `HOST`/`PORT`
env vars by `from_env`, defaulting to `0.0.0.0:3000`). `auth.rs` defines
`auth_middleware`, an axum `middleware::from_fn` handler that gates every
route behind a bearer token when one is configured. `envelope.rs` defines the
`Envelope`/`EnvelopeKind` pair used only by the aether-facing route.
`routes.rs` holds the non-session handlers — `health`, `ready`, `invoke`
(stateless), `aether_invoke` (envelope-wrapped) — and their request types
(`InvokeRequest`). `session_routes.rs` holds the five session-lifecycle
handlers (`create_session`, `send_message`, `list_messages`, `get_session`,
`end_session`) and their request/query types, plus `store_err_status`, which
maps `SessionMemoryError` variants to HTTP status codes shared by several
handlers. `openapi.rs` holds `openapi_json`, a hand-written OpenAPI 3.1 document served at
`/openapi.json`.

`build_router` constructs one `Arc<RateLimiter>` (`RateLimiter::new(100,
60)`, from [Guardrails](guardrails.md)) and attaches it as an
`axum::Extension` shared across the whole router. Routes are declared twice:
once nested under `/v1` (`v1_router`, itself nesting `v1_session_router`
under `/sessions`), and again at the router root for backward compatibility
(`/health`, `/ready`, `/invoke`, `/aether/invoke`, plus `/openapi.json` which
exists only at the root). Both trees share the same handler functions and
both take `State<Arc<Agent>>` directly — there is no intermediate
`AppState`/`SessionState` wrapper struct. Three layers wrap the whole router,
applied in this order: `Extension(rate_limiter)`, then
`CorsLayer::permissive()`, then `middleware::from_fn(auth::auth_middleware)`.

## Runtime Flows

**Startup: `with_http_server()` → `spawn_server` lifecycle:**
1. `Agent::builder(...).with_http_server()` only sets
   `AgentBuilder.enable_http_server = true`; no I/O happens yet.
2. `AgentBuilder::build()` constructs the `Agent`, wraps it in `Arc`, calls
   `spawn_background_workers`, and only then — behind `#[cfg(feature =
   "http")]` and gated on `enable_http_server` — calls
   `crate::http::spawn_server(Arc::clone(&agent))`.
3. `spawn_server` reads `HttpConfig::from_env()` and the `API_KEY`/
   `ALLOW_INSECURE` env vars, then calls `validate_bind_security`
   synchronously; a violation panics immediately, before `build()` returns —
   a fatal HTTP configuration error surfaces at agent-construction time, not
   on the first incoming request.
4. `build_router(agent)` assembles the `Router`; `spawn_server` then
   `tokio::spawn`s a task that binds a `TcpListener` (panicking on a bind
   failure) and runs `axum::serve(listener, router)`. `spawn_server` itself
   returns immediately, so `AgentBuilder::build()` hands back a live
   `Arc<Agent>` while the listener keeps running in the background for as
   long as the process holds that `Arc` and the tokio runtime is alive.

**A request through a session route → `Agent::invoke` → response:**
1. `POST /v1/sessions/:session_id/messages` dispatches to `send_message`,
   which pulls `Extension<Arc<RateLimiter>>` and calls
   `limiter.check(&req.user_id)` before touching the request body; a
   rate-limit error short-circuits with `StatusCode::TOO_MANY_REQUESTS`.
2. An empty `message` (after `.trim()`) returns `StatusCode::BAD_REQUEST`;
   otherwise `send_message` calls `agent.invoke(&req.user_id, session_id,
   &req.message)` — [Agent](agent.md)'s full turn-taking path (skill
   routing, memory assembly, strategy execution, HITL interception if
   configured).
3. `Ok(reply)` maps to `200 OK` with `{ session_id, reply }`. `Err(AgentError)`
   is matched by variant: `AgentError::Session` goes through
   `store_err_status` (`SessionMemoryError::NotFound` → `404`, `Database` →
   `500`), `AgentError::Llm` → `502 BAD_GATEWAY`, `AgentError::Skill`/
   `AgentError::Json` → `500`.
4. The stateless sibling (`POST /invoke` → `invoke` in `routes.rs`) runs the
   same rate-limit-then-empty-check sequence but calls
   `agent.invoke_stateless(&request.message)`, skipping session/memory/skill
   context entirely. `POST /aether/invoke` (`aether_invoke`) wraps the same
   `invoke_stateless` call but takes and returns an `Envelope`: it checks
   `env.kind == EnvelopeKind::Invoke` (else `400`), reads
   `env.payload["input"]`, and echoes the original `id`/`metadata` back
   inside an `EnvelopeKind::Result` (success) or `EnvelopeKind::Error`
   (failure) envelope — unlike `invoke` and `send_message`, `aether_invoke`
   never consults the `RateLimiter`.

**Auth decision: `API_KEY` / `ALLOW_INSECURE` / loopback rule:**
1. At startup, `spawn_server` calls `validate_bind_security(host,
   api_key_set, allow_insecure)`: it returns `Ok(())` immediately if
   `api_key_set`, `allow_insecure`, or `is_loopback_host(host)` is true, and
   otherwise returns an `Err` that `spawn_server` turns into a panic — a
   non-loopback bind cannot start without an explicit `API_KEY` or
   `ALLOW_INSECURE=true`.
2. `is_loopback_host` treats `"localhost"` (case-insensitively) and any
   address whose `IpAddr::is_loopback()` is true as loopback; an unparseable
   hostname is conservatively treated as non-loopback.
3. `api_key_configured` (startup guard) and `get_api_key` (per-request check,
   `auth.rs`) both apply the same rule independently: `API_KEY` counts as set
   only if present and non-empty after `.trim()`, so `API_KEY=""` behaves as
   unset in both places.
4. Per request, `auth_middleware` — installed as the outermost of the three
   router layers — reads `get_api_key()` (cached in a `OnceLock`, read once
   per process): if a key is configured, the `Authorization` header must be
   exactly `Bearer <key>` (trimmed) or the request gets `401 UNAUTHORIZED`
   before reaching routing; if no key is configured, every request passes
   through unauthenticated.

## Key Decisions

Newest first.

### Fail-closed non-loopback bind; empty `API_KEY` counts as unset everywhere
- **Decision** — `validate_bind_security` refuses to start the server on a
  non-loopback host unless `API_KEY` is non-empty or `ALLOW_INSECURE=true`
  is set; `api_key_configured` and `get_api_key` both treat a present-but-empty
  or whitespace-only `API_KEY` as unset.
- **Context** — the 2026-07-02 architecture-review design spec's "Secure-by-default
  HTTP" section specifies exactly this rule: "If bind host is non-loopback
  **and** `API_KEY` is unset **and** `ALLOW_INSECURE != "true"` → panic at
  startup with an actionable message," plus "Loopback detection: parse host
  as `IpAddr` and use `is_loopback()`; treat `"localhost"` as loopback;
  unparseable hostnames are treated as non-loopback (conservative)." PR #24's
  body adds the empty-key motivation: "empty `API_KEY` now counts as unset in
  both the guard and the auth middleware (previously `API_KEY=""` half-enabled
  auth that `Bearer ` bypassed)."
- **Alternatives rejected** — none recorded.
- **Consequences** — `HttpConfig::from_env`'s default host is `0.0.0.0`
  (non-loopback), so out of the box `spawn_server` panics unless an operator
  sets `API_KEY`, `ALLOW_INSECURE=true`, or binds explicitly to a loopback
  `HOST`; PR #24's Breaking-changes section lists exactly this: "HTTP
  sidecar refuses unauthenticated non-loopback binds".
- **Ref** — 2026-07-03, PR #24 (commit `9cb41d6`).

### `/v1`-prefixed routes with backward-compatible root aliases; per-user rate limiting; `/openapi.json`
- **Decision** — `build_router` registers every route twice: nested under
  `/v1` and again at the router root, sharing the same handlers; a shared
  `Arc<RateLimiter>` is attached as a router-wide `Extension` and checked
  first by `invoke`, `send_message`, and `list_messages`; a hand-written
  OpenAPI 3.1 document is served at `/openapi.json`.
- **Context** — PR #22's body lists this as one of its architecture changes:
  "HTTP API: `/v1` prefix with backward-compat root aliases, 429 rate
  limiting, `/openapi.json` endpoint" and "Session routes: `list_messages`
  handler; `send_message` enforces rate limit."
- **Alternatives rejected** — none recorded.
- **Consequences** — the versioned and legacy route trees are two separate
  `Router` values built from identical `.route(...)` calls rather than one
  tree mounted at two paths, so adding a new route while preserving the root
  alias means updating both call sites in `build_router`; a rate-limited
  request returns `429` before the request body is validated or the `Agent`
  is touched at all.
- **Ref** — 2026-06-14, PR #22 (commits `34a80d9`, `6b3d731`).

### `avs-server` absorbed into `avs-agent`'s `http` feature; the `Agent` owns the sidecar
- **Decision** — the standalone `avs-server` binary crate is deleted; all
  HTTP capability moves into `avs-agent/src/http/` behind the optional `http`
  Cargo feature, route handlers take `State<Arc<Agent>>` directly, and the
  server is spawned from inside `AgentBuilder::build()` — after the `Agent`
  already exists as an `Arc` — rather than the reverse.
- **Context** — the commit's own message states the change directly: "Routes
  now take `Arc<Agent>` directly as axum state (no `AppState`/`SessionState`
  wrappers). `avs-server` crate deleted; all HTTP capability lives in
  `avs-agent/src/http/` behind the optional `http` cargo feature." This is
  consistent with the http registry design spec (untracked)'s framing that
  "An agent runs without aether" — an `Agent` must be constructible and
  runnable with no HTTP surface at all, which the feature-gate plus explicit
  `with_http_server()` opt-in preserves.
- **Alternatives rejected** — none recorded; the commit describes the
  refactor directly rather than weighing options.
- **Consequences** — `spawn_server` has no lifecycle of its own: it is called
  exactly once, inside `build()`, after the `Agent` already exists, and
  returns immediately after handing a `tokio::spawn`ed task a clone of
  `Arc<Agent>`. Nothing in current source constructs the router or binds the
  listener independently of `AgentBuilder::build()`, so the sidecar cannot
  outlive or be built before its owning `Agent` — the spawned task holds only
  a strong `Arc` clone, keeping the `Agent` alive for as long as the listener
  runs, never the other way around.
- **Ref** — 2026-05-25, commit `2529eb3`.

### Aether inbound compatibility is envelope-only
- **Decision** — `/aether/invoke` accepts an `Envelope{kind: Invoke}` and
  returns an `Envelope{kind: Result | Error}` carrying the same `id` and
  `metadata`.
- **Context** — the endpoint is a compatibility boundary for callers that
  exchange Aether envelopes. It invokes the already-constructed `Agent`
  through `invoke_stateless`; no registry lifecycle client is part of this
  HTTP module.
- **Consequences** — both `/aether/invoke` and `/v1/aether/invoke` remain
  available. A successful invocation returns an `EnvelopeKind::Result`; an
  agent failure returns `500` with an `EnvelopeKind::Error`.
- **Ref** — `avs-agent/src/http/routes.rs` (`aether_invoke`).

## Implementation Notes

- Layer order in `build_router` matters: `Extension(rate_limiter)` is applied
  first, then `CorsLayer::permissive()`, then
  `middleware::from_fn(auth::auth_middleware)` — the last `.layer()` call
  becomes the outermost layer in axum/tower's composition, so
  `auth_middleware` runs first on every request, ahead of CORS. A CORS
  preflight (`OPTIONS`) request with no `Authorization` header is therefore
  rejected with `401` by `auth_middleware` before `CorsLayer::permissive()`
  ever sees it, whenever `API_KEY` is set.
- `auth_middleware` is one blanket layer over the entire router — both the
  `/v1` and legacy route trees, plus `/openapi.json` — so `/health` and
  `/ready` are also gated behind the bearer token whenever `API_KEY` is set;
  there is no per-route auth exemption.
- `spawn_server` panics rather than returning a `Result` on a
  `validate_bind_security` failure or a `TcpListener::bind` failure; its own
  comment states the tradeoff: "Deliberate startup panic: `Agent::new` has no
  error channel (until the builder lands), and failing loudly beats silently
  serving unauthenticated." `AgentBuilder` has since landed (PR #26), but
  `spawn_server`'s panic-on-failure behavior was not revisited afterward.
- `auth.rs` opens with the header comment `// avs-server/src/auth.rs`, a
  leftover from before the `avs-server` → `avs-agent` absorption; the file no
  longer lives at that path.
- `list_messages`'s `before` cursor filters against `sequence_num`, an index
  computed by `enumerate()` over the full history returned by
  `agent.load_messages`, not a persisted column — pagination correctness
  depends on that method returning a stable, gap-free ordering across calls.

## Source Anchors

- `avs-agent/src/http/mod.rs`
- `avs-agent/src/http/auth.rs`
- `avs-agent/src/http/config.rs`
- `avs-agent/src/http/envelope.rs`
- `avs-agent/src/http/routes.rs`
- `avs-agent/src/http/session_routes.rs`
- `avs-agent/src/http/openapi.rs`
- `avs-agent/src/agent/builder.rs`
- `avs-agent/Cargo.toml`
- `avs-agent/` (crate)

## Related Pages

- [Agent](agent.md)
- [Guardrails](guardrails.md)
- [Session](session.md)
- [Core Runtime](core-runtime.md)
