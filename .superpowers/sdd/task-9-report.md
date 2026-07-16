# Task 9 Report: Maintained McpLoader Path

## Status

Complete. `examples/mcp-demo` now constructs `McpServerConfig` for its
in-process endpoint and loads it through `McpLoader::load`. No dependency was
added from `avs-agent` to `avs-mcp`.

## RED Evidence

The integration test was written before changing the demo. Its first run in
the restricted sandbox failed while binding the real in-process listener:

```text
cargo test -p agentverse-mcp --test loader_integration_test -- --nocapture
...
Operation not permitted
```

This is an environment failure, not a loader assertion. With local-listener
permission, the same test was already green because `McpLoader::load` itself
was implemented before this task. The missing Task 9 behavior was its
maintained call site: before the edit, `examples/mcp-demo` directly composed
`McpTransport`, `McpClient`, and `McpCatalogSource`, with no `McpLoader::load`
reference. No artificial regression was introduced solely to manufacture a
functional RED for existing loader behavior.

## GREEN Evidence

`avs-mcp/tests/loader_integration_test.rs` deserializes a minimal
`[[mcp_servers]]` wrapper, starts an in-process `McpServer` on an ephemeral
localhost port, and calls `McpLoader::load` into a fresh registry. It proves
that one MCP-category `echo` adapter is registered and executes through the
remote server. A second test proves a missing Streamable HTTP URL remains a
typed `McpError::Config`.

Final verification:

- `cargo test -p agentverse-mcp -- --nocapture`: passed: 11 tests passed and
  doc-tests passed. The local-listener permission was used for server tests.
- `cargo check -p example-mcp-demo`: passed.
- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.

## Self-Review

- `examples/mcp-demo` preserves its existing in-process server setup and now
  calls `McpLoader::load` with a `McpServerConfig` endpoint.
- `McpClient`, `McpTransport`, and `McpCatalogSource` remain publicly exported
  for advanced use.
- `avs-agent/Cargo.toml` has no `agentverse-mcp` dependency.
- `wiki/mcp.md` describes the demo as the maintained loader path and removes
  the stale dead-code claim.
- No Stage 4 clippy or layering command was run, per the Task 9 instruction.

## Concerns

None for Task 9. Local listener tests require elevated local socket access in
this execution environment.
