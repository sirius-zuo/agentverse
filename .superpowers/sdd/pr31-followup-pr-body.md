# PR #31 Framework Gaps Follow-Up

## Summary

This follow-up closes the ten real framework gaps identified during PR #31's
internal-wiki verification and reconciles the affected wiki pages with the
implemented runtime behavior.

## Original Gaps Closed

- [x] **1. HITL trust boundary:** build policy only from trusted system skills, so user-supplied skill metadata cannot create or replace HITL gates. Fixes: `ad73450`, `68140ed`.
- [x] **2. Native tool-calling request path:** thread active tool definitions from the runner through the registry and ReAct loop to provider requests. Fixes: `9b3e717`, `c06a94f`, `75cc734`.
- [x] **3. ActionGuard runtime ownership:** deprecate the unused compatibility guard in favor of the already-wired `HitlContext` and `ToolRegistry::execute_many_hitl` path. Fix: `dfb634b`.
- [x] **4. StrategyRouter integration:** let `AgentBuilder` accept a router and select a bounded strategy per invocation. Fixes: `9cbb02f`, `92fbf2a`.
- [x] **5. Dead MCP adapter:** remove the unreferenced pre-rewrite `avs-mcp/src/tools.rs` adapter. Fix: `603c612`.
- [x] **6. Legacy core tracing scaffold:** remove unused tracer implementations and their obsolete dependency path while retaining the metrics facade. Fix: `54baf3c`.
- [x] **7. Half-dead outbound Aether client:** remove unused outbound registration/event methods while preserving inbound `/aether/invoke`. Fix: `6420a3f`.
- [x] **8. Unwired MCP loader:** use `McpLoader::load` in the maintained MCP demo, with documentation clarifying the example-owned configuration path. Fixes: `baf68ff`, `1ee517c`.
- [x] **9. Subagent composition-root drift:** add `AgentBuilder::with_subagent_executor` and make `spawn_subagent` registration atomic, idempotent, and first-registration-wins. Fixes: `37511b6`, `ed8291e`, `913f419`.
- [x] **10. Stale HITL sweep-worker comment:** correct the queue comment to name `HitlSweepWorker`. Fix: `e313f37`.

## Intentional Deferral

Native response-side tool parsing remains intentionally deferred. This change
wires request-side tool definitions to supported providers but retains the
existing text-response parsing path until provider response types expose a
stable native tool-call representation.

## Documentation

The corresponding internal-wiki source anchors were reconciled in `c1f32b0`.

## Verification

- `./scripts/check-file-sizes.sh` (exit 0)
- `./scripts/check-layering.sh` (exit 0)
- `cargo fmt --all --check` (exit 0)
- `cargo clippy --all -- -D warnings` (exit 0)
- `cargo check --workspace --all-features` (exit 0)
- `cargo test --workspace` reached the `httpmock` tests but exits 101 in this
  sandbox because local `127.0.0.1:0` listener binding is prohibited. Rerun
  that command in a controller environment that permits local test servers.
