# Core Runtime

## Purpose

`avs-core` is the foundation crate: it owns the `ModelProvider` abstraction and
its three built-in implementations, the `ConnectionManager` that turns a
provider into a reliable HTTP client (retries, circuit breaker, fallback), the
`ProviderRegistry` that resolves a provider by name, `LlmRunner` as the
callable entry point strategies invoke, `PromptRegistry` for minijinja
template rendering, and the `Config`/`ProviderConfig`/`AgentError` types every
other crate builds on. It exists as a separate, dependency-free crate because
it sits at the bottom of the workspace's layering (`scripts/check-layering.sh`
places it in Layer 0): every other `avs-*` crate depends on it, and it depends
on none of them. Its job is to make one LLM call reliably and return a
structured response — everything above it (strategy loops, memory, tools) is
built in terms of the types it exports.

Its maintained observability surface is `metrics`, which provides the shared
OpenTelemetry `record_*` facade; workspace logging is provided separately by
`avs-logging`. The former `avs-core` tracing scaffold and its legacy feature
are removed rather than retained as an inactive API.

## Position in the System

`avs-core` consumes no other workspace crate — it is the dependency-free
foundation. It is consumed by essentially every other crate: the data/state
layer (`avs-hitl`, [memory](memory.md), [session](session.md),
`avs-integration`), the tools/safety layer ([tools](tools.md),
[guardrails](guardrails.md), [mcp](mcp.md)), the strategy layer
([strategy](strategy.md), `avs-react`, `avs-plan`, `avs-router`,
[subagent](subagent.md)), and the composition root, [agent](agent.md)
(`avs-agent`), which wires an `LlmRunner` into every `Agent` it builds.
[Subagent](subagent.md) additionally calls `ConnectionManager::with_model`
directly for per-subagent model overrides, and the strategy crates render
prompts through `PromptRegistry` and construct `GenerateRequest` values that
flow into `LlmRunner::invoke`, `invoke_with_tools`, or `invoke_structured`.

## Architecture

```mermaid
classDiagram
    class ModelProvider {
        <<trait>>
        +name() str
        +build_request(model, GenerateRequest) Value
        +parse_response(body) GenerateResponse
        +request_headers(api_key) HeaderMap
        +endpoint_path(model) String
    }
    class AnthropicProvider
    class OpenAICompatible
    class GeminiProvider
    ModelProvider <|.. AnthropicProvider
    ModelProvider <|.. OpenAICompatible
    ModelProvider <|.. GeminiProvider

    class ProviderRegistry {
        +with_builtins() ProviderRegistry
        +register(name, ProviderFactory)
        +build(name, settings) ResolvedProvider
    }
    class ResolvedProvider {
        +provider Box~dyn ModelProvider~
        +api_base String
        +api_key String
        +model_name String
    }
    ProviderRegistry --> ResolvedProvider : build()

    class ConnectionManager {
        +from_config(ProviderConfig, ProviderRegistry) ConnectionManager
        +with_model(model_name, ProviderRegistry) ConnectionManager
        +with_fallback(ConnectionManager) ConnectionManager
        +generate(GenerateRequest) GenerateResponse
    }
    ConnectionManager --> ModelProvider : Box~dyn~
    ConnectionManager ..> ProviderRegistry : resolves via

    class LlmRunner {
        +from_config(Config) LlmRunner
        +invoke(messages) GenerateResponse
        +invoke_with_tools(messages, tools) GenerateResponse
        +invoke_structured(messages, schema) GenerateResponse
    }
    LlmRunner --> ConnectionManager : Arc~ConnectionManager~

    class GenerateRequest {
        +system Option~String~
        +messages Vec~Message~
        +tools Option~Vec~ToolDefinition~~
        +response_format Option~Value~
    }
    class GenerateResponse {
        +content Vec~ContentBlock~
        +usage UsageStats
    }
    class ContentBlock {
        <<enum>>
        Text
        ToolUse
        ToolResult
    }
    GenerateResponse ..> ContentBlock : content
    ModelProvider ..> GenerateRequest : consumes
    ModelProvider ..> GenerateResponse : produces
```

`ModelProvider` is a pure, synchronous protocol translator: `build_request`
turns a `GenerateRequest` into the provider's own wire JSON, and
`parse_response` turns a raw response body back into a `GenerateResponse`.
Neither method touches the network — that separation is deliberate, so each
provider's request/response shape is unit-testable without an HTTP mock.
`ConnectionManager` is the piece that actually owns the `reqwest::Client`, the
circuit breaker, retry/backoff state, and an optional boxed `fallback`
(`ConnectionManager::with_fallback`); it is constructed either directly
(`ConnectionManager::anthropic`/`openai`/`gemini` convenience constructors) or
via `ConnectionManager::from_config`, which asks a `ProviderRegistry` to
resolve a `ProviderConfig { name, settings }` into a `ResolvedProvider`
(provider instance plus `api_base`/`api_key`/`model_name`). `LlmRunner` is the
thin entry point strategy crates call: it holds an `Arc<ConnectionManager>`
and exposes `invoke`, `invoke_with_tools`, and `invoke_structured`, all implemented through a shared
`invoke_inner` that splits `System`-role messages out of the conversation and
assembles a `GenerateRequest`. Both `Message` (`avs-core/src/memory/mod.rs`)
and `GenerateResponse` carry `content: Vec<ContentBlock>` rather than a flat
string — `ContentBlock` is a three-variant enum (`Text { text }`,
`ToolUse { id, name, input }`, `ToolResult { tool_use_id, content, is_error }`)
shared by every provider; `Message::as_text`/`GenerateResponse::as_text`
(via the shared `content_as_text` helper) flatten it back to a string —
`Text` verbatim, `ToolUse`/`ToolResult` as a short bracketed summary — for
callers (guardrails, memory, logging) that only need text.
`invoke_with_tools` forwards native tool definitions to the provider request
path and, on the way back, surfaces every `tool_use`/`tool_calls` entry the
provider returned as a `ContentBlock::ToolUse` value inside
`GenerateResponse.content`; it does not itself dispatch a tool call or run a
tool loop — that dispatch, and feeding the result back as a
`ContentBlock::ToolResult` block on the next turn, is the strategy loop's
job (see [Strategy](strategy.md)).
`PromptRegistry` (in `prompt.rs`) is a separate,
decoupled subsystem: a minijinja `Environment` pre-loaded with default
templates (`system`, `react`, `strategies.*`, `router`), optionally extended
from a directory of `.j2`/`.toml` files or a `PromptConfig`. `LlmRunner` does
not use `PromptRegistry` itself — strategy crates render templates through it
to build the `system` string and `Example`s they pass into a `GenerateRequest`.

## Runtime Flows

**`invoke` (unstructured):**
1. Caller passes `Vec<Message>` to `LlmRunner::invoke`.
2. `invoke_inner` partitions messages: `MessageRole::System` entries are
   joined into `system`, the rest stay as `messages` in order.
3. A `GenerateRequest { system, messages, tools: None, response_format: None }`
   is built and passed to `ConnectionManager::generate`.
4. `generate` checks the circuit breaker, then calls
   `provider.build_request(model_name, request.clone())` to get the wire body,
   `provider.endpoint_path`/`provider.request_headers` for the URL and headers.
5. The retry loop (see below) sends the HTTP request; on success
   `provider.parse_response` produces a `GenerateResponse` with `UsageStats`.
6. `LlmRunner::invoke` returns `Result<GenerateResponse, AgentError>`,
   mapping `ModelError` into `AgentError::Model`.

**`invoke_structured` (schema-constrained):**
1. Caller passes `messages` plus a `serde_json::Value` JSON schema to
   `LlmRunner::invoke_structured`.
2. `invoke_inner` runs identically to the unstructured path, except
   `response_format` is `Some(schema)`.
3. Each provider encodes the schema differently at `build_request` time:
   `AnthropicProvider` emits `output_config.format` with
   `type: "json_schema"`; `OpenAICompatible` wraps it as
   `response_format: { type: "json_schema", json_schema: { name, schema } }`;
   `GeminiProvider`'s `GeminiRequest` has no `response_format` field at all, so
   the schema is silently not enforced server-side for Gemini.
4. The rest of `ConnectionManager::generate` (retry, circuit breaker,
   fallback, `parse_response`) is unchanged from the unstructured path.

**`invoke_with_tools` (native tool definitions):**
1. Caller passes `messages` plus `Vec<ToolDefinition>` to
   `LlmRunner::invoke_with_tools`.
2. `invoke_inner` partitions messages as above and builds a `GenerateRequest`
   with `tools: Some(tools)` and `response_format: None`.
3. Each provider serializes the tool definitions into its own wire format
   with `strict: true` set unconditionally (`AnthropicTool`/
   `FunctionDefinition`); any `ContentBlock::ToolUse`/`ContentBlock::ToolResult`
   blocks already present in the conversation (from a prior turn) are
   serialized into the same request — `AnthropicProvider` maps them 1:1 onto
   `tool_use`/`tool_result` content blocks, `OpenAICompatible` collects
   `ToolUse` blocks into a message's `tool_calls` array and expands each
   `Tool`-role `ToolResult` block into its own `role: "tool"` message.
4. `ConnectionManager::generate` sends that request through its normal retry,
   circuit-breaker, and fallback path.
5. Each provider's `parse_response` collects every `text`/`tool_use`
   (Anthropic) or `tool_calls` (OpenAI-compatible) entry in the response into
   `GenerateResponse.content` as `ContentBlock::Text`/`ContentBlock::ToolUse`
   values — a response with neither is a hard `ModelError::InvalidResponse`,
   and a malformed `tool_use`/`tool_calls` entry (missing `id`/`name`/`input`,
   or unparseable `arguments` JSON) is a hard error rather than silently
   dropped, so a real tool call can't vanish and leave only an unrelated
   final answer.
6. ReAct's normal and HITL loops (`ReActStrategy::invoke_with_active_tools`)
   use this entry point when their active tool names resolve to at least one
   registry definition, falling back to `invoke` (preserving `tools: None`
   rather than sending `Some([])`) when none resolve; they parse the returned
   `ContentBlock::ToolUse` entries into `CycleAction::ToolCalls`. Dispatching
   those calls to a `Tool` implementation and feeding the results back as
   `ContentBlock::ToolResult` blocks on the next turn is owned by the ReAct
   loop, not this crate — see [Strategy](strategy.md).

**Retry, circuit breaker, and fallback inside `generate`:**
1. Before building the request, `generate` takes the circuit breaker lock; if
   `CircuitState::Open` and the timeout hasn't elapsed, and a `fallback` is
   configured, it recurses into `fallback.generate(request)`; otherwise it
   returns `ModelError::CircuitOpen`.
2. On each HTTP attempt: a transport error or non-2xx status calls
   `circuit_breaker.record_failure()`; a `429` uses `rate_limit_backoff_ms`
   (4x the normal exponential backoff) or the server's `Retry-After` header,
   clamped to 60s via `clamp_retry_after_ms`; other failures use `backoff_ms`
   (exponential, capped at a shift of 10).
3. If attempts remain, the loop sleeps and retries. If retries are exhausted
   or a non-429/5xx failure occurs and a `fallback` is configured, the request
   is retried once against the fallback's own `generate` (which runs the same
   circuit-breaker/retry logic independently). Otherwise the last `ModelError`
   is returned.
4. A successful parse calls `circuit_breaker.record_success()`, records
   `UsageStats` via `crate::metrics::record_llm_call` ([observability](observability.md)),
   and returns the `GenerateResponse`.

## Key Decisions

### Dead `{% if tools %}`/`Action:`/`Answer:` free-text instructions dropped from default prompt templates
- **Decision** — `DEFAULT_SYSTEM_TEMPLATE` drops its `{% if tools %}...{% else %}...{% endif %}` block (which rendered `Action:`/`Action Input:`/`Answer:` free-text-format instructions) down to three static sentences with no conditional; `DEFAULT_REACT_TEMPLATE`'s `Current conversation:`/`{{ conversation }}`/`Available tools:`/`{{ tools }}` preamble and its `Thought:`/`Action:`/`Action Input:` response-format instructions are replaced with one sentence stating that tool calls are handled natively.
- **Context** — commit `0d675d4`'s message: "`DEFAULT_SYSTEM_TEMPLATE` no longer instructs the deleted free-text Answer:/Action: protocol on its default (no-tools-context) render path, which every agent without a custom `system.j2` hits on every invoke" — a Phase 6 final-review finding. Commit `840ed65` made the equivalent fix to `DEFAULT_REACT_TEMPLATE` and 6 example prompts, since the free-text parser these instructions targeted (`avs-react/src/parse.rs`) had already been deleted in Phase 5 — see [Strategy](strategy.md).
- **Alternatives rejected** — none recorded.
- **Consequences** — supersedes the "flat prompt strings" Key Decision's Consequences note below, which stated `DEFAULT_SYSTEM_TEMPLATE` gained a `{{ tools }}` block; that block existed from PR #1 until this change, and `DEFAULT_SYSTEM_TEMPLATE` no longer renders one. `DEFAULT_PLAN_AND_EXECUTE_TEMPLATE` keeps its own `{{ tools }}` block unchanged — this cleanup targeted only the two templates the deleted free-text ReAct parser consumed (`system`, `react`).
- **Ref** — 2026-08-02, commits `840ed65` and `0d675d4` (PR #35).

### `GeminiProvider` hard-errors on tool-bearing content instead of silently degrading
- **Decision** — `GeminiProvider::build_request` now returns `ModelError::InvalidResponse` when any message contains a `ContentBlock::ToolUse`/`ContentBlock::ToolResult` block, joining its pre-existing hard error for `request.tools.is_some()`, instead of letting those blocks silently flatten through `Message::as_text` into a `[tool_use: ...]`/`[tool_result ...]` summary string sent as plain conversation text.
- **Context** — PR #35's body: "`GeminiProvider` (out of scope for native tool calling) now hard-errors instead of silently degrading."
- **Alternatives rejected** — none recorded.
- **Consequences** — Gemini stays usable for text-only conversations (`invoke`, `invoke_structured`, with the latter's existing schema-not-enforced caveat below unaffected); any caller that routes a tool-bearing conversation through `GeminiProvider` now gets a request-time `ModelError` instead of a response built from a lossy text summary of tool calls the model never actually issued as free-text `Action:` output.
- **Ref** — 2026-08-01, commit `6f9e6ed` (PR #35).

### Native tool-call round-tripping: `Message`/`GenerateResponse` content becomes `Vec<ContentBlock>`; providers gain request+response tool-call serialization
- **Decision** — `Message.content` and `GenerateResponse.content` change from `String` to `Vec<ContentBlock>` (`Text`/`ToolUse`/`ToolResult`); `AnthropicProvider` and `OpenAICompatible` send every tool definition with `strict: true`, parse native `tool_use`/`tool_calls` entries out of a provider response into `ContentBlock::ToolUse` values, and serialize `ContentBlock::ToolUse`/`ContentBlock::ToolResult` blocks already present in the conversation back onto the request instead of only ever flattening them to text.
- **Context** — PR #35's stated root cause: the free-text `Action:` convention only rendered one level of a tool's `input_schema.properties` and never dereferenced `$ref`/`definitions`, so any tool with a nested-object parameter rendered a blank parameter description — the direct cause of the `business-report` example's crash (`Invalid arguments: missing field 'likely_weeks'`). Native tool calling sends the full JSON Schema through each provider's own strict-mode `tools` field instead, so the model never has to guess.
- **Alternatives rejected** — a degraded free-text fallback mode for a provider/model without native tool-calling support, rejected outright per the PR body: "This is a hard requirement, no fallback... a request-time error, not a degraded text-based mode."
- **Consequences** — a malformed `tool_use`/`tool_calls` entry (missing `id`/`name`/`input`, or unparseable `arguments` JSON) is now a hard `ModelError` in both providers rather than silently dropped; `avs-core/src/tool.rs`'s `ToolCall`/`ToolCallResult` gained a provider-issued `id: String` that must round-trip as the matching `ToolResult`'s `tool_use_id` (that file is anchored on [Tools](tools.md), not duplicated here — see Implementation Notes); `avs-react`'s free-text parser (`avs-react/src/parse.rs`) was deleted in the same PR — see [Strategy](strategy.md).
- **Ref** — 2026-07-28, PR #35, commits `d893597`, `ba293da`, `1906e8a`, `2013387`, `0d7a306`, `b8c94e4`.

### Open `ProviderRegistry` replacing the closed `ProviderConfig` enum
- **Decision** — `ProviderConfig` becomes `{ name: String, settings: HashMap<String, String> }` with ergonomic constructors (`::anthropic`, `::openai`, `::gemini`, `::custom`); a new `ProviderRegistry` (name-keyed table of `ProviderFactory` closures, never global state) is the single dispatch point `ConnectionManager::from_config`/`with_model` call through.
- **Context** — the 2026-07-02 architecture review flagged the provider seam as the least open/closed part of the design: adding a provider meant editing the `ProviderConfig` enum, `LlmRunner::from_config`'s match, `ConnectionManager::from_config`'s match, and `with_model`'s string match, all in lockstep.
- **Alternatives rejected** — runtime `.so`/WASM plugin loading (registration stays compile-time Rust, a provider author writes and registers a factory function, but no `avs-core` edit); config-file-driven definition of a brand-new provider (a YAML file can select a registered provider by name, it cannot define a new one).
- **Consequences** — 61 call sites migrated across 24 files; `Config::validate()` no longer catches a missing `model_name`/`api_key` (that check moved into each factory as `ModelError::MissingSetting`/`InvalidApiKey`, surfacing at `ConnectionManager::from_config` time instead); `ConnectionManager::from_config`/`with_model` gained a `&ProviderRegistry` parameter (breaking, but `LlmRunner::from_config`'s own signature is unchanged since it builds `ProviderRegistry::with_builtins()` internally).
- **Ref** — 2026-07-04, PR #27.

### `with_model` returns `Result`, shares circuit breaker, and honors `Retry-After`
- **Decision** — `ConnectionManager::with_model` returns `Result<Self, ModelError>` (new `ModelError::UnknownProvider`) instead of panicking via `unreachable!()`; a `with_model`-derived instance shares its parent's `circuit_breaker` `Arc` rather than getting an independent one; `429` responses honor a server `Retry-After` header (clamped to 60s) with a 4x-backoff fallback when absent; API keys are validated as legal HTTP header values at construction (`ModelError::InvalidApiKey`).
- **Context** — a whole-branch architecture-review audit found `with_model` panicking on an unrecognized provider name, per-model overrides bypassing an already-open circuit breaker for the same endpoint/key, and rate-limit backoff not respecting server-provided delay hints.
- **Alternatives rejected** — leaving the panic behind a "should never happen" comment (an audit-level defect, not acceptable); independent circuit breakers per model override (would let one model's override keep hammering an endpoint whose circuit the primary model already tripped).
- **Consequences** — every `with_model` caller (`avs-subagent`'s `SubAgentExecutor`, this crate's own tests) must handle a `Result`; retry/backoff math is exercised directly by unit tests (`backoff_is_exponential_and_capped`, `rate_limit_backoff_is_4x`, `clamp_retry_after_ms_caps_at_60s`).
- **Ref** — 2026-07-03, PR #24.

### Server-enforced structured output, encoded per provider
- **Decision** — `GenerateRequest` gains `response_format: Option<serde_json::Value>`; `LlmRunner::invoke_structured(messages, schema)` is added alongside `invoke`, both sharing `invoke_inner`; each provider maps the schema to its own wire shape at `build_request` time, rather than the runner normalizing a single format.
- **Context** — strategies needed schema-constrained decoding (e.g. planning steps) without hand-rolling a different wire shape per provider at every call site. PR #23 introduced the field and the OpenAI-compatible encoding (`response_format: { type: "json_schema", json_schema: { name, schema } }`, the shape vLLM/llama.cpp constrained decoding expects); commit `e0720fb` followed with the Anthropic encoding (`output_config.format` with `type: "json_schema"`).
- **Alternatives rejected** — a single shared wire encoding across providers: Anthropic's API takes `output_config.format`, not OpenAI's `response_format` field (per commit `e0720fb`'s message), so the encoding has to live inside each provider's `build_request`.
- **Consequences** — `invoke` and `invoke_structured` share one code path with no duplicated request-splitting logic; existing `GenerateRequest { .. }` struct literals across the workspace needed `..Default::default()` once the struct gained a field. `GeminiProvider` does not consume `response_format` at all — `GeminiRequest` has no such field, so the schema is silently dropped for Gemini (DEVELOPMENT.md's structured-output table documents this as "Not supported — free text returned"). No PR or spec records a rationale for that gap; it is observed current state, not a documented scoping decision.
- **Ref** — 2026-06-24, PR #23 and commit `e0720fb`.

### `GenerateRequest`/`GenerateResponse` replace flat prompt strings; caching stays provider-internal
- **Decision** — `ModelProvider::build_request`/`parse_response` take/return structured `GenerateRequest` (`system`, `messages`, `tools`) and `GenerateResponse` (`content`, `usage`) instead of a flat `prompt: &str`; each provider applies its own caching strategy internally, invisible to callers.
- **Context** — the flat-string design made correct Anthropic `system`-field usage and prompt caching structurally impossible, and discarded message roles before they reached the provider.
- **Alternatives rejected** — exposing cache-control decisions to callers (rejected outright — the design's explicit goal was that "caching logic stays internal to each provider"); non-text content blocks (images, documents) — deferred as a non-goal for a later change.
- **Consequences** — `UsageStats` (`input_tokens`, `output_tokens`, `cache_write_tokens`, `cache_read_tokens`) is added, accumulates via `AddAssign`, and is surfaced to callers through `CycleResult.total_usage`; `AnthropicProvider` places `cache_control: ephemeral` on the last tool, the system block, and the penultimate conversation message; `DEFAULT_SYSTEM_TEMPLATE` gained a `{{ tools }}` block so the cached system prefix carries the tool list too. (Superseded 2026-08-02 — see "Dead `{% if tools %}`/`Action:`/`Answer:` free-text instructions dropped from default prompt templates" above: `DEFAULT_SYSTEM_TEMPLATE` no longer renders a `{{ tools }}` block.)
- **Ref** — 2026-05-16, PR #1.

## Implementation Notes

- `ModelProvider` implementations must stay free of HTTP/IO — `build_request`
  and `parse_response` are synchronous and side-effect-free by design, which is
  what lets `provider_build_request_for_test` (a `#[doc(hidden)]` test-only
  method on `ConnectionManager`) exercise wire-format logic without a mock
  server.
- `ProviderRegistry` is a plain struct, not global state: every caller
  (production code and tests alike) constructs its own via `new()` or
  `with_builtins()`, so there is no shared mutable registry to leak state
  across parallel test execution.
- `Config::validate()` only checks that `provider.name` is non-empty; it
  cannot catch a missing `model_name`/`api_key` for an arbitrary registered
  provider, since it has no `ProviderRegistry` to consult. Real callers are
  unaffected because `LlmRunner::from_config` calls `validate()` immediately
  before `ConnectionManager::from_config` (which does the registry-backed
  check) in the same function — but calling `validate()` alone and treating
  success as "provider is fully configured" is a latent trap for a new caller.
- `PromptRegistry::has_react_template()` reflects whether a `react.j2` file
  was loaded from a prompts directory; this is a decoupling seam consumed by
  `avs-react`'s cycle logic to decide whether to prime a one-time preamble
  message, not something `avs-core` itself acts on.
- `OpenAICompatible::new()` reads `LLAMA_DISABLE_THINKING` once at
  construction (defaulting `disable_thinking` to `true`) to keep ReAct-style
  structured-text output reliable against reasoning-capable OpenAI-compatible
  backends; it is not re-read per request.
- Metrics remain the core crate's maintained observability path. It creates
  OTel instruments only through `avs-core/src/metrics.rs`; binaries install
  exporters, while `avs-logging` owns `tracing` subscriber initialization.
- `AnthropicContentBlock`'s `text()`/`cache_control()` accessors (in
  `anthropic_provider.rs`) are gated `#[cfg(test)]` — they exist only so
  tests can pin down the system-prompt/cache-marker wiring, not as part of
  the provider's runtime path (commit `ad7ef31`).
- `avs-core/src/tool.rs` defines `ToolCall`/`ToolCallResult`/`ToolHandle`
  and the `Tool`/`ErasedTool` traits, physically inside this crate, but this
  page does not anchor that file: [Tools](tools.md) already lists
  `avs-core/src/tool.rs` in its own Source Anchors and documents those types
  as part of the tool-calling contract `avs-tools`' `ToolRegistry` builds on.
  Anchoring it in both pages would duplicate ownership of the same drift
  contract.
- Known follow-ups explicitly deferred out of scope (per PR #24's body):
  parsing `Retry-After` in HTTP-date form (only integer-seconds is handled
  today) and `--all-targets` on the CI clippy job. Per PR #27: registering a
  genuinely new provider still requires a Rust `ProviderFactory` function —
  there is no runtime plugin loading, and a YAML `Config` can only *select* an
  already-registered provider by name, not define one.

## Source Anchors

- `avs-core/src/model.rs`
- `avs-core/src/model/connection_manager.rs`
- `avs-core/src/model/registry.rs`
- `avs-core/src/model/anthropic_provider.rs`
- `avs-core/src/model/openai_compatible.rs`
- `avs-core/src/model/gemini_provider.rs`
- `avs-core/src/llm_runner.rs`
- `avs-core/src/memory/mod.rs`
- `avs-core/src/prompt.rs`
- `avs-core/src/config.rs`
- `avs-core/src/builder.rs`
- `avs-core/src/example.rs`
- `avs-core/src/error.rs`

## Related Pages

- [Agent](agent.md)
- [Strategy](strategy.md)
- [Subagent](subagent.md)
- [Guardrails](guardrails.md)
- [Observability](observability.md)
- [Memory](memory.md)
- [Session](session.md)
- [Tools](tools.md)
- [MCP](mcp.md)
- [Eval and Test Infra](eval-and-test-infra.md)
- [HTTP Sidecar](http-sidecar.md)
- [Integration](integration.md)
