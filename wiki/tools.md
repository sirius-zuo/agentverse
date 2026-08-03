# Tools

## Purpose

`avs-tools` (crate `agentverse-tools`) is where every capability an agent can
invoke — arithmetic, wall-clock time, filesystem globbing, HTTP calls, web
search, shell execution, and meta-discovery over the tool set itself — is
implemented and exposed through one registry. It turns `avs-core`'s
`Tool`/`ErasedTool` trait pair into a concrete runtime: `ToolRegistry` is the
single place a `Tool` implementation becomes callable by name and describable
to an LLM as a JSON schema or a strict-mode native `ToolDefinition`, with
async dispatch (single and parallel), BM25 keyword search for on-demand
discovery via `FindToolsTool`, and (with `avs-hitl`) approval interception
before a risky call executes. It sits in Layer 2 alongside `avs-guardrails`
and `avs-mcp`, so every reasoning strategy above it shares one tool-calling
contract regardless of which loop drives it.

## Position in the System

`avs-tools` consumes [Core Runtime](core-runtime.md) (`avs-core`) for the
`Tool`/`ErasedTool` trait pair, `ToolError`/`ToolResult`,
`ToolCall`/`ToolCallResult`/`ToolHandle`/`ToolDefinition`, and the `metrics`
facade it records tool-call outcomes through, and [HITL](hitl.md) (`avs-hitl`,
via `agentverse::hitl::HitlHook`) for the approval hook `execute_many_hitl`
checks before dispatch. It depends on nothing else in the workspace.

It is consumed by: [Strategy](strategy.md) (`avs-react`'s `CycleSkeleton`/
`ReActStrategy` and `avs-plan` each hold an `Arc<ToolRegistry>`; ReAct resolves
`active_tool_names` per invocation through `tool_definitions_for` for the
provider request, while `PlanStrategy`/`HierarchicalStrategy` call
`tool_summaries_for` for a text hint — see Runtime Flows); [Agent](agent.md)
(`avs-agent`, top-level holder of the `Arc<ToolRegistry>` built at
`AgentBuilder` time, and caller of `restricted_to_names` to scope a routed
strategy's registry to one invocation's `active_tool_names`);
[SubAgent](subagent.md) (`SubAgentExecutor` calls `filter_by_names` to build
a scoped registry per run, and `register_if_absent` to register
`SubAgentTool` under `SPAWN_SUBAGENT_TOOL_NAME`, exactly once even under
concurrent construction); and [MCP](mcp.md) (`avs-mcp`'s `McpToolAdapter`
implements `ErasedTool` directly and registers via `register_erased`, and
`McpServer` serves a registry's `schema()`/`execute()` back out over the MCP
protocol). [Skill](skill.md) documents how a `Skill`'s `tools: Vec<String>`
field becomes the `active_tool_names` slice `avs-agent`'s `invoke` passes
into `RunStrategy::run_with_active_tools`; this page covers only what
`avs-tools` does with that slice once received.

## Architecture

```mermaid
classDiagram
    class Tool {
        <<trait>>
        +Args JsonSchema + DeserializeOwned
        +name() str
        +description() str
        +execute(Args) ToolResult
    }
    class ErasedTool {
        <<trait>>
        +schema() Value
        +execute_raw(Value) ToolResult
    }
    class ToolRegistry {
        -tools RwLock~ToolMap~
        -index RwLock~BM25Index~
        +new() Arc~Self~
        +register(tool)
        +register_with_options(tool, ToolOptions)
        +register_erased(Arc~dyn ErasedTool~, ToolOptions)
        +register_if_absent(tool) bool
        +execute(name, args) ToolResult
        +execute_many(Vec~ToolCall~) Vec~ToolCallResult~
        +execute_many_hitl(calls, hook) Result
        +spawn_tool(ToolCall) ToolHandle
        +schema() Vec~Value~
        +tool_definitions_for(names) Result~Vec~ToolDefinition~, StrictSchemaError~
        +search(query, limit) Vec~ToolInfo~
        +filter_category(category) Arc~ToolRegistry~
        +filter_by_names(names) Arc~ToolRegistry~
        +restricted_to_names(names) Arc~ToolRegistry~
        +tool_summaries() String
        +tool_summaries_for(names) String
    }
    class ToolOptions {
        +category Option~String~
        +execution_mode ExecutionMode
    }
    class ExecutionMode {
        <<enum>>
        Inline
        Background
    }
    class StrictSchemaError {
        <<enum>>
        OpenDictionary
        AmbiguousOptionalRef
    }
    class ActiveToolSet {
        -names HashSet~String~
        +all(registry) Self
        +activate(names)
        +deactivate(names)
        +schemas(registry) Vec~Value~
        +contains(name) bool
    }
    class BM25Index {
        -docs Vec
        -df HashMap
        +insert(id, text)
        +search(query, limit) Vec
    }
    class ToolInfo {
        +name String
        +description String
        +schema Value
        +score f32
    }
    class FindToolsTool {
        -registry Arc~ToolRegistry~
    }
    class ShellTool {
        -workdir PathBuf
        -timeout_duration Duration
        -blocked Vec~String~
    }
    Tool <|.. Calculator
    Tool <|.. DateTimeTool
    Tool <|.. FileSearch
    Tool <|.. HttpClient
    Tool <|.. WebSearch
    Tool <|.. ShellTool
    Tool <|.. FindToolsTool
    Tool ..|> ErasedTool : blanket impl
    ToolRegistry o-- ToolOptions
    ToolRegistry ..> BM25Index
    ToolRegistry ..> ErasedTool : Arc~dyn~
    ToolRegistry ..> StrictSchemaError : tool_definitions_for()
    ActiveToolSet ..> ToolRegistry : schemas()
    FindToolsTool --> ToolRegistry : search()
```

`Tool` (`avs-core/src/tool.rs`) is the trait every built-in tool implements:
an associated `Args` type (`JsonSchema + DeserializeOwned`), and `execute`
taking the already-typed `Args`. Because a trait with an associated type
isn't object-safe, `ErasedTool` is the sealed, object-safe shadow the registry
actually stores — a blanket `impl<T: Tool> ErasedTool for T` derives `schema()`
from `T::Args` via `schemars::gen::SchemaGenerator` and implements
`execute_raw` by deserializing the incoming `Value` into `T::Args` (returning
`ToolError::InvalidArgs` on failure) before calling `Tool::execute`. Tool
authors never implement `ErasedTool` directly.

`ToolRegistry` (`registry.rs`) owns a `RwLock<HashMap<String, (Arc<dyn
ErasedTool>, ToolOptions)>>` plus a `RwLock<BM25Index>` kept in sync on every
insert. `ToolRegistry::new()` returns an `Arc<Self>` with `FindToolsTool`
already registered against that same `Arc`, so `find_tools` is present in
every registry without caller action. `register`/`register_with_options`
accept any `T: Tool`, erase it, and index `"{name} {description}"` into the
BM25 index; `register_erased` takes a pre-erased `Arc<dyn ErasedTool>` for
non-`Tool` implementors such as `avs-mcp`'s `McpToolAdapter`; `register_if_absent`
does the same insert as a single map-write-lock check-and-insert, returning
whether it actually inserted — `avs-subagent`'s `SubAgentExecutor` uses it so
the root `SubAgentTool` can't be double-registered by a race. `ToolOptions`
carries an optional `category` string (consumed only by `filter_category`)
and an `ExecutionMode` (`Inline` or `Background`) attached at registration
time, not on the tool implementation — the same tool type can be registered
inline in one registry and background in another. `execute` looks up one
tool and calls `execute_raw`; `execute_many` fans a `Vec<ToolCall>` out over
a `tokio::task::JoinSet` and collects `ToolCallResult`s in completion order,
each result carrying the originating call's `id` — the provider-issued
identifier `ToolCall`/`ToolCallResult` gained in `avs-core::tool` to
correlate a result back to its `ToolUse` block (see [Core Runtime](core-runtime.md));
`execute_many_hitl` layers a `HitlHook::check_tool` pre-check in front of
`execute_many` (see [HITL](hitl.md)) and returns `Err(HitlInterruptResult)`
before executing anything if any call in the batch needs approval.
`filter_category`, `filter_by_names`, and `restricted_to_names` all build a
fresh `ToolRegistry` sharing the same `Arc<dyn ErasedTool>` instances through
a shared private `copy_named_tools` helper; `filter_by_names` always excludes
`SPAWN_SUBAGENT_TOOL_NAME` (how `avs-subagent` guarantees a SubAgent can never
spawn a nested SubAgent), while `restricted_to_names` — `avs-agent`'s tool
for scoping a routed strategy's registry to exactly one invocation's active
tool set — does not. `tool_definitions_for(names)` walks the requested names
in caller-provided order, ignores unknown names, and for each selected tool
runs `ErasedTool::schema()["input_schema"]` through
`strict_schema::to_strict_schema` before mapping the result into an
`avs-core` `ToolDefinition`; it returns `Result<Vec<ToolDefinition>,
StrictSchemaError>` rather than skipping an unstrictifiable tool, "since
silently dropping a tool from the list offered to the model is its own kind
of silent failure" (doc comment). `avs-react`'s `ReActStrategy` maps an `Err`
here to `AgentError::Config(ConfigError::Invalid(..))` (see Key Decisions).

`ActiveToolSet` (`active.rs`) is a plain `HashSet<String>` wrapper independent
of `ToolRegistry`'s own storage: `schemas(&registry)` calls
`registry.schema()` and filters it to names in the set. **It is now
orphaned** — PR #35 moved this prompt-shaping job to `tool_definitions_for`/
`tool_summaries_for` (below); see Implementation Notes for the verification
and PR body wording.

`BM25Index` (`search.rs`) is a from-scratch BM25 implementation (`k1=1.5`,
`b=0.75`) over tokenized `"{name} {description}"` text, with no external
search dependency. `FindToolsTool` (`find_tools.rs`) wraps `registry.search`
behind the `Tool` trait so the LLM can call it like any other tool; it takes
`{query, limit}` (`limit` defaulting to 5) and returns each hit as a
`ToolInfo` (`name`, `description`, `score`).

`ShellTool` (`shell.rs`) runs commands via `sh -c` inside a fixed `workdir`
under a `tokio::process::Command`, wrapped in `tokio::time::timeout` with
`kill_on_drop(true)`, after splitting the command with `shell_words::split`
to reject an empty command and check the first token against a
caller-supplied `blocked` list. The other five built-ins —
`Calculator` (`calculator.rs`), `DateTimeTool` (`datetime.rs`), `FileSearch`
(`file_search.rs`, glob-based), `HttpClient` (`http_client.rs`, scheme-checked
async reqwest; its `headers` argument is `Vec<HeaderPair>`, not a `HashMap`
— see Key Decisions), and `WebSearch` (`web_search.rs`, DuckDuckGo HTML
scrape plus page-text fetch via `scraper`) — are each a single `struct` plus
a `#[derive(Deserialize, JsonSchema)] Args` struct, no shared base type
beyond `Tool` itself.

## Runtime Flows

**Typed `Tool<Args>` → `ErasedTool` dispatch:**
1. A tool author implements `Tool` for their struct with a
   `#[derive(Deserialize, JsonSchema)]` `Args` type; the blanket `ErasedTool`
   impl gives it `schema()` and `execute_raw()` for free.
2. `ToolRegistry::register` erases the tool into `Arc<dyn ErasedTool>`,
   inserts it into the map keyed by `name()` alongside a `ToolOptions`, and
   indexes `"{name} {description}"` into the `BM25Index`.
3. `ToolRegistry::execute(name, args)` clones the `Arc`, calls
   `execute_raw(args)` (which deserializes `args` into the concrete `Args`
   type and calls `Tool::execute`), and records the outcome via
   `agentverse::metrics::record_tool_call`. `execute_many` does the same for
   a batch, dispatched concurrently through a `JoinSet` rather than routing
   through `execute` at all.

**Native tool-definition resolution per invocation** (replaces the pre-PR-#35
`ActiveToolSet` filtering flow that occupied this slot — `ActiveToolSet` is
now orphaned, see Implementation Notes):
1. `avs-react`'s `ReActStrategy::invoke_with_active_tools` calls
   `ToolRegistry::tool_definitions_for(active_tool_names)` fresh on every
   iteration — the names come from `avs-agent`'s `invoke`, ultimately a
   skill's `tools` field (see [Skill](skill.md)).
2. For each requested name that resolves, `tool_definitions_for` runs its
   `input_schema` through `strict_schema::to_strict_schema`, which forces
   `additionalProperties: false` and a complete `required` list onto every
   object node, rejecting the schema with `StrictSchemaError::OpenDictionary`
   or `AmbiguousOptionalRef` rather than guessing (see Key Decisions).
3. On `Ok`, ReAct sends the definitions via `LlmRunner::invoke_with_tools`
   when non-empty, or `LlmRunner::invoke` when empty — preserving
   `GenerateRequest.tools: None` over `Some([])`. On `Err`, `ReActStrategy`
   maps it to `AgentError::Config(ConfigError::Invalid(..))`.
4. `PlanStrategy`/`HierarchicalStrategy` never call `tool_definitions_for`;
   they call `tool_summaries_for(active_tool_names)` for a text hint instead
   (see [Strategy](strategy.md)) — strict-mode native definitions are a
   ReAct-only path today. Either way, `ToolRegistry::execute`/`execute_many`
   accept any registered tool name regardless of what was offered to the
   model: restricting what is *shown* is separate from restricting what can
   be *called* (`filter_by_names`/`restricted_to_names`).

**`find_tools` progressive discovery:**
1. `ToolRegistry::new()` registers `FindToolsTool` against the registry it
   returns, before any caller-supplied tool — every registry has `find_tools`
   from construction.
2. The LLM calls `find_tools` with `{query, limit}`; `FindToolsTool::execute`
   calls `registry.search(query, limit)`, which delegates to
   `BM25Index::search` over the name+description text indexed at each
   `register` call.
3. Results come back as `ToolInfo` (`name`, `description`, `score`) in the
   tool's `ToolResult` — the LLM reads them from its next observation and can
   call a newly-learned tool name by name in a subsequent turn. `find_tools`'s
   results are not automatically folded into any active-tool-name list a
   strategy is using; execution succeeds regardless, since `execute` never
   checks what was offered to the model in the first place.

## Key Decisions

Newest first.

### Tool schemas are strictified into a fail-closed native format; open dictionaries and ambiguous optional refs are rejected, not guessed
- **Decision** — `strict_schema::to_strict_schema` recursively transforms a
  `schemars`-generated JSON Schema into the strict dialect both Anthropic and
  OpenAI-compatible native tool-calling require — `additionalProperties:
  false` and a complete `required` list on every object node, including
  nodes reachable only via `definitions` or a `oneOf`/`anyOf`/`allOf`
  combinator — returning `StrictSchemaError` rather than a best-effort schema
  when a shape can't be made strict without changing its meaning:
  `OpenDictionary` for an arbitrary-key object, since "forcing
  `additionalProperties: false` onto it would collapse it to one that can
  only ever produce `{}`"; `AmbiguousOptionalRef` for an optional bare-`$ref`/
  `allOf`-wrapped field with no null signal, since `schemars` produces "this
  identical shape" for a genuine `Option<T>` and a non-nullable
  `#[serde(default)]` field alike, so "the two are indistinguishable from the
  schema alone" and it's "rejected rather than guessed" (module doc comment).
- **Context** — PR #35's root cause: the old free-text tool-listing renderer
  "only rendered one level of a tool's `input_schema.properties` and never
  dereferenced `$ref`/`definitions`, so any tool with a nested-object
  parameter... rendered a **blank** parameter description" that the model
  then guessed at — the direct cause of the `business-report` example's
  crash. Native tool-calling sends the schema itself, but every provider's
  strict mode additionally requires the closed-object shape this produces.
- **Alternatives rejected** — guessing nullability for an ambiguous bare-`$ref`
  field instead of rejecting it (per the doc comment quoted above); a
  fallback to the old free-text rendering for an unstrictifiable schema
  (PR #35: native tool-calling is "a hard requirement, no fallback").
- **Consequences** — `http_client`'s `headers` argument was redesigned from
  `HashMap<String, String>` to `Vec<HeaderPair>` (commit `69111ad`) because an
  arbitrary-key map is exactly the `OpenDictionary` shape this rejects; any
  future dictionary-shaped tool argument needs the same redesign.
  `tool_definitions_for` is fallible as a direct result (see Architecture) —
  an unstrictifiable `Args` type surfaces as a `StrictSchemaError` on first
  request, not a schema silently rendered wrong.
- **Ref** — 2026-07-28, PR #35 (Phase 2), commit `5adb1de`.

### `ToolRegistry` instrumented at three sites, not one, after discovering `execute_many` bypasses `execute`
- **Decision** — `ToolRegistry::execute`, `execute_many`, and the HITL
  interception path in `execute_many_hitl` are each instrumented separately
  with `agentverse::metrics::record_tool_call`, recording `"<unknown>"`
  rather than the raw name for not-found calls.
- **Context** — while instrumenting the three stable crate boundaries
  (LLM connection, tool registry, HITL queues), the PR discovered that
  `execute_many` "does **not** funnel through `execute`" — missing this
  "would have silently under-counted every agent-driven tool call, since
  `avs-agent` only calls the `_many` variants."
- **Alternatives rejected** — recording the raw, potentially
  LLM-hallucinated tool name on not-found paths was identified as "an
  unbounded-cardinality violation of the facade's own rule" during a
  whole-branch review and fixed before merge, not shipped and revisited.
- **Consequences** — `agentverse.tool.calls` and `agentverse.tool.duration`
  carry a bounded `tool.name` label set and an `outcome` label including
  `hitl_intercepted`; a tool-call metric now exists regardless of which
  dispatch path (`execute`, `execute_many`, or the HITL-checked variant) an
  agent-driving strategy uses.
- **Ref** — 2026-07-04, PR #25.

### `find_tools` excluded from `McpServer`'s `tools/list`
- **Decision** — `avs-mcp`'s `McpServer` filters `find_tools` out of its
  `tools/list` response even though `ToolRegistry::new()` auto-registers it.
- **Context** — `find_tools` searches the *local* registry; before this fix,
  an MCP client connecting to an AgentVerse-backed `McpServer` would discover
  3 tools when the operator had only registered 2, because `find_tools`
  "is meaningless to an MCP client and should never cross the MCP boundary."
- **Alternatives rejected** — none recorded; the PR body states the fix
  directly rather than weighing alternatives (e.g. not auto-registering
  `find_tools` for MCP-exposed registries).
- **Consequences** — a `ToolRegistry`'s tool count as seen by a local LLM
  (which does see `find_tools`) and as seen by a remote MCP client (which
  does not) legitimately differ by exactly one; no other filtering is
  applied to the MCP-exposed tool list.
- **Ref** — 2026-06-02, PR #6.

### BM25 keyword search behind a semantic-search-shaped interface
- **Decision** — tool discovery is BM25 keyword search implemented from
  scratch in `avs-tools`, exposed as `ToolRegistry::search(query, limit) ->
  Vec<ToolInfo>` and wrapped in the `FindToolsTool` meta-tool, which
  `ToolRegistry::new()` registers automatically.
- **Context** — the tools-architecture-refactor design spec (untracked)
  frames this as solving "no dynamic tool discovery" as the tool count
  grows, so the agent isn't forced to receive every tool schema at session
  start.
- **Alternatives rejected** — the spec does not implement embedding-based
  search now, but states the interface is "intentionally identical to what
  a semantic/embedding-backed search would expose — the implementation can
  be swapped without changing callers," i.e. a deliberate placeholder rather
  than a considered-and-rejected alternative.
- **Consequences** — `BM25Index` indexes only `"{name} {description}"` text
  at registration time (no re-indexing hook), and callers depending on
  `ToolInfo.score` ordering are coupled to BM25's ranking today even though
  the function signature would tolerate a different ranking method later.
- **Ref** — 2026-05-27, PR #5.

### `Tool<Args>` associated-type trait + `ErasedTool` shim replaces `AsyncTool`
- **Decision** — the single `AsyncTool` trait (hand-written `parameters()`
  returning `serde_json::Value`) was replaced by `Tool` with an associated
  `Args: JsonSchema + DeserializeOwned` type, schemas derived automatically
  via `schemars`, plus a sealed `ErasedTool` trait (blanket-implemented for
  any `T: Tool`) to keep the registry's storage object-safe.
- **Context** — the tools-architecture-refactor design spec (untracked)
  states the prior approach was "verbose, error-prone, and disconnected from
  the tool's actual argument parsing," since every tool hand-built its JSON
  schema.
- **Alternatives rejected** — the spec explicitly rules out reintroducing a
  separate sync-tool trait or adapter: "The framework remains async-only...
  No `SyncTool` trait, no `SyncToolAdapter`," citing Rig (described in the
  spec as "the leading Rust agent framework") as taking the same approach.
- **Consequences** — every built-in tool now pairs a `#[derive(Deserialize,
  JsonSchema)]` `Args` struct with its `Tool` impl instead of a manual
  `parameters()` method; malformed LLM-supplied arguments surface as the new
  `ToolError::InvalidArgs` variant from inside the blanket `execute_raw`
  rather than an ad hoc extraction error per tool.
- **Ref** — 2026-05-27, PR #5.

### `ShellTool`'s sandbox is a working-directory jail plus a blocklist, not a filesystem sandbox
- **Decision** — `ShellTool` enforces exactly three things: commands start in
  a configured `workdir`, a per-call timeout, and a caller-supplied
  `blocked` binary-name list. It does not prevent a command from leaving
  `workdir` once running.
- **Context** — an early revision's doc comment overstated this as a
  "sandboxed shell tool" with a "workdir jail"; a follow-up commit corrected
  the docs to state plainly that `workdir` "is NOT a filesystem sandbox —
  absolute paths, symlinks, and `cd` commands inside the shell can still
  access the full filesystem," recommending pairing with OS-level isolation
  for stronger guarantees.
- **Alternatives rejected** — no rationale is recorded for not implementing
  a real filesystem/kernel sandbox (e.g. containers, seccomp, chroot) at this
  layer; the commit's scope was correcting the documentation and adding
  `kill_on_drop(true)`, not redesigning enforcement.
- **Consequences** — `ShellTool` is suitable only for trusted-model,
  dev-tool-style deployments per its own doc comment; any operator relying on
  `workdir` alone as an escape-proof boundary is relying on behavior the tool
  explicitly disclaims.
- **Ref** — 2026-05-18, commit `199407d`.

## Implementation Notes

- `ToolRegistry` uses `std::sync::RwLock`, not `tokio::sync::RwLock` — every
  lock acquisition (`self.tools.read().unwrap()`, etc.) is synchronous and
  held only across a `HashMap` operation or an `Arc` clone, never across an
  `.await`.
- `filter_category` has no call site outside `avs-tools/tests/` — examples
  pass a `category` into `ToolOptions`, but nothing in `avs-agent`,
  `avs-react`, or `avs-plan` calls `filter_category` to act on it.
  `filter_by_names` (used by `avs-subagent`) and `restricted_to_names` (used
  by `avs-agent` for routed-strategy construction) are the filtering paths
  actually exercised in production code.
- `ExecutionMode::Background` and `ToolRegistry::spawn_tool`/`ToolHandle`
  exist as a defined-but-unused fire-and-forget interface — the tools
  design spec (untracked) describes this as "defined now, implemented
  later"; no strategy in the workspace currently dispatches through
  `spawn_tool` or checks `ExecutionMode` before calling `execute`/
  `execute_many`.
- **`ActiveToolSet` (`active.rs`) is orphaned dead code.** PR #35 moved its
  prompt-shaping job to `tool_definitions_for`/`tool_summaries_for`; a
  workspace grep for `ActiveToolSet`/`.schemas(` finds no caller outside this
  crate's own `lib.rs` re-export and a doc comment in `avs-skill/src/types.rs`.
  PR #35's "Known follow-ups": "left in place, not deleted, since removing
  public API surface was out of scope." (`avs-react`'s
  `CycleSkeleton::execute_tool` is orphaned for the same reason — anchored on
  [Strategy](strategy.md), not here.)
- `tool_summaries`/`tool_summaries_for` build a required-args hint string by
  reading `schema()["input_schema"]["properties"]`/`["required"]` directly
  rather than through a typed schema model. Since PR #35, this is
  `avs-plan`'s format only (`PlanStrategy`/`HierarchicalStrategy`'s
  per-active-tool-set prompts, see [Strategy](strategy.md)) — `avs-react`'s
  ReAct loop no longer renders a text tool summary at all; it sends
  `tool_definitions_for`'s native, strict-mode definitions instead.

## Source Anchors

- `avs-tools/src/lib.rs`
- `avs-tools/src/registry.rs`
- `avs-tools/src/active.rs`
- `avs-tools/src/strict_schema.rs`
- `avs-tools/src/search.rs`
- `avs-tools/src/find_tools.rs`
- `avs-tools/src/shell.rs`
- `avs-tools/src/calculator.rs`
- `avs-tools/src/datetime.rs`
- `avs-tools/src/file_search.rs`
- `avs-tools/src/http_client.rs`
- `avs-tools/src/web_search.rs`
- `avs-core/src/tool.rs`
- `avs-tools/` (crate)

## Related Pages

- [Core Runtime](core-runtime.md)
- [HITL](hitl.md)
- [Strategy](strategy.md)
- [SubAgent](subagent.md)
- [MCP](mcp.md)
- [Skill](skill.md)
- [Agent](agent.md)
- [Observability](observability.md)
