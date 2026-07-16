# Task 5 Report: ReAct Active Tool Definitions

## Scope

- Base: `c06a94f`
- Commit subject: `feat(react): pass active tool definitions to runner`
- Changed only ReAct request dispatch, focused tests, and the three required wiki pages.
- `prepare_buffer_with_active`, `ToolRegistry::tool_summaries_for`, ReAct's
  existing prose prompt rendering, and `parse_response` remain intact.
- Plan and Hierarchical strategies were not changed.
- Native tool-call response parsing remains deferred.

## RED Evidence

Command:

```text
cargo test -p agentverse-react -- --nocapture
```

Result: failed with exit code 101 before implementation.

Expected Task 5 failures:

```text
run_with_active_tools_forwards_non_empty_definitions ... FAILED
thread 'run_with_active_tools_forwards_non_empty_definitions' panicked:
active tool definitions must be sent

run_hitl_forwards_non_empty_active_tool_definitions ... FAILED
thread 'run_hitl_forwards_non_empty_active_tool_definitions' panicked:
active tool definitions must be sent
```

The empty-resolution test already passed in RED:

```text
run_with_empty_or_unknown_active_tools_sends_none ... ok
```

The same sandboxed run also failed the pre-existing
`run_hitl_returns_interrupted_with_typed_history_and_pending_calls` test because
its HTTP mock could not bind `127.0.0.1:0` (`Operation not permitted`). This was
an environment failure, not a Task 5 assertion. The final full test gate was
rerun with local-listener permission and passed.

## GREEN Evidence

Focused command:

```text
cargo test -p agentverse-react --test react_test active_tool -- --nocapture
```

Result: passed with exit code 0.

```text
running 3 tests
test run_hitl_forwards_non_empty_active_tool_definitions ... ok
test run_with_active_tools_forwards_non_empty_definitions ... ok
test run_with_empty_or_unknown_active_tools_sends_none ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 13 filtered out
```

The recording provider proves that:

- Normal ReAct forwards the resolved `echo` definition.
- HITL ReAct forwards the same non-empty definition set.
- Empty and all-unknown active sets reach the provider as
  `GenerateRequest.tools == None`, never `Some([])`.

## Implementation

`ReActStrategy::invoke_with_active_tools` resolves the requested names through
`ToolRegistry::tool_definitions_for`. It calls `LlmRunner::invoke_with_tools`
only when that output is non-empty; otherwise it calls `LlmRunner::invoke`.
Both `run_with_active_tools` and `run_hitl` use this shared helper for every
model request. Existing text prompt preparation and response parsing are
unchanged.

## Stage 2 Gate Evidence

### ReAct tests

```text
cargo test -p agentverse-react -- --nocapture
```

Passed with exit code 0: 19 unit tests, 16 integration tests, and 0 doc tests;
35 total tests passed, 0 failed.

### Formatting

```text
cargo fmt --all --check
```

Passed with exit code 0 and no output.

### Layering

```text
scripts/check-layering.sh
```

Passed with exit code 0:

```text
No layer-direction violations found.
```

### Clippy

```text
cargo clippy --all -- -D warnings
```

Passed with exit code 0 across the workspace; finished the `dev` profile with
no warnings.

## Self-Review

- Confirmed both normal and HITL loops use the shared request helper.
- Confirmed empty and all-unknown resolution uses `invoke`, preserving `None`.
- Confirmed non-empty definitions retain registry-selected ordering and data.
- Confirmed `prepare_buffer_with_active` and `parse_response` are unchanged.
- Confirmed no Plan or Hierarchical strategy files changed.
- Confirmed wiki wording does not claim native response parsing or full native
  tool calling.
- `git diff --check` passed.
- No correctness findings or remaining implementation concerns.
