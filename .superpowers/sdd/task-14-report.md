# Task 14 Report: Add AgentBuilder SubAgent Executor Integration

## Status

Complete. `AgentBuilder::with_subagent_executor` accepts an
`Arc<SubAgentExecutor>` and registers `spawn_subagent` in the builder's shared
`ToolRegistry` during `build`, before constructing `Agent`.

## RED/GREEN TDD

### RED

Added `agent_builder_with_subagent_executor_registers_spawn_subagent_tool` in
`avs-agent/tests/agent_test.rs`. It builds an `Agent` with the new builder API
and observes the shared `ToolRegistry`.

Command:

```text
cargo test -p agentverse-agent agent_builder_with_subagent_executor_registers_spawn_subagent_tool -- --nocapture
```

Result before implementation: failed with `E0599`, because
`AgentBuilder::with_subagent_executor` did not exist.

### GREEN

Added optional `Arc<SubAgentExecutor>` builder state, the chainable
`with_subagent_executor` method, and one conditional
`SubAgentExecutor::register_tool` call before `Agent` construction. `Agent`
does not retain the executor; the registered `SubAgentTool` owns its `Arc`.

Re-ran the focused command: passed, with the new test observing
`spawn_subagent` in the shared registry after `build`.

The renamed lower-level test,
`subagent_executor_register_tool_registers_spawn_subagent_tool`, continues to
cover `SubAgentExecutor::register_tool` directly.

## Documentation

- Updated `wiki/agent.md` with the high-level builder path and ownership
  boundary.
- Updated `wiki/subagent.md` to describe both the high-level builder and
  lower-level `register_tool` paths, replacing stale design claims.
- Updated the tracked `DEVELOPMENT.md` API summary.

## Verification

- `cargo test -p agentverse-agent subagent -- --nocapture`: passed (2 tests).
- `cargo test -p agentverse-subagent -- --nocapture`: passed (22 tests).
  The sandboxed attempt could not bind `httpmock` loopback listeners; the
  identical command passed when rerun with local-listener permission.
- `cargo fmt --all --check`: passed.
- `scripts/check-layering.sh`: passed.
- `cargo clippy --all -- -D warnings`: passed.
- `git diff --check`: passed during self-review.

## Self-Review

No findings. The change is limited to the builder integration, focused tests,
and stale documentation; no-executor behavior remains unchanged.

## Concerns

None. The subagent test suite requires permission to bind localhost test ports
in this sandboxed environment.

## Review Follow-Up: Idempotent Builder Registration

### Finding

`AgentBuilder::build` called `SubAgentExecutor::register_tool` whenever an
executor was configured, even when `spawn_subagent` was already registered.
`ToolRegistry` replaced the named tool in its map but appended another BM25
document, so lower-level registration followed by builder registration, or
multiple agent builds over one registry, produced duplicate search results.

### RED

Added failing-first coverage for all reported cases:

- `agent_builder_after_lower_level_registration_has_one_spawn_subagent_search_result`
  called `SubAgentExecutor::register_tool` before the builder and observed two
  searchable `spawn_subagent` results instead of one.
- `agent_builder_does_not_overwrite_pre_registered_spawn_subagent_tool`
  registered a sentinel implementation and observed two search results; the
  builder had also replaced the named tool in the registry map.
- `multiple_agent_builders_share_one_spawn_subagent_search_result` built two
  agents over one registry and observed two search results instead of one.

RED commands:

```text
cargo test -p agentverse-agent agent_builder_ -- --nocapture
cargo test -p agentverse-agent multiple_agent_builders_share_one_spawn_subagent_search_result -- --nocapture
```

Each new regression test failed with an observed result count of `2` versus
the expected `1`.

### GREEN

The builder now checks `ToolRegistry::has_tool(SPAWN_SUBAGENT_TOOL_NAME)` and
calls the lower-level registration method only when the name is absent. This
defines first-registration-wins behavior at the builder boundary without
changing `ToolRegistry` replacement semantics.

Focused GREEN command:

```text
cargo test -p agentverse-agent spawn_subagent -- --nocapture
```

Result: passed all five builder and lower-level `spawn_subagent` tests.

### Follow-Up Verification

- `cargo test -p agentverse-agent subagent -- --nocapture`: passed (5 tests).
- `cargo test -p agentverse-subagent -- --nocapture`: passed (22 tests).
- `cargo fmt --all --check`: passed.
- `scripts/check-layering.sh`: passed.
- `cargo clippy --all -- -D warnings`: passed.
- `git diff --check 37511b6`: passed during self-review.

### Follow-Up Self-Review

No findings. The behavior change is confined to `AgentBuilder`; the tests
prove first-registration-wins and one searchable document, and
`ToolRegistry` itself is unchanged.
