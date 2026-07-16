# Task 12 Report: Remove Unused Outbound Aether Client Surface

## Status

Completed against base `54baf3c`.

## Implementation

- Deleted `avs-agent/src/http/aether_client.rs`, including the unused
  `AetherClient`, registration response types, and outbound
  `register`/`deregister`/`push_event` operations.
- Removed the private module declaration from `avs-agent/src/http/mod.rs`.
- Removed `reqwest` from `agentverse-agent`'s optional `http` feature and
  dependency list. Cargo refreshed `Cargo.lock`; only the
  `agentverse-agent` package dependency entry changed.
- Kept the inbound `aether_invoke` handler and both `/aether/invoke` and
  `/v1/aether/invoke` registrations unchanged.
- Updated `wiki/http-sidecar.md` to describe Aether as an inbound envelope
  compatibility boundary only, removed the outbound client from the diagram
  and source anchors, and removed the obsolete outbound-debt discussion.

## Regression Coverage

Strengthened the route tests with deterministic in-process strategies:

- A successful `Invoke` envelope returns `200` with a `Result` envelope on
  both root and `/v1` aliases, preserving `id` and `metadata`.
- A failing agent returns `500` with an `Error` envelope, preserving `id` and
  `metadata` and carrying the error payload.
- A non-`Invoke` envelope continues to return `400`.

### Dead-Code-Removal Test Strategy

No artificial RED test was created for removing `AetherClient`: it had no
callers, so a test of the deleted outbound behavior would preserve a surface
that the task explicitly removes. The replacement regression coverage proves
the retained public HTTP behavior at the route boundary, and compilation plus
the full workspace checks prove the removed private module has no remaining
references.

## Verification Evidence

All commands exited successfully:

- `cargo test -p agentverse-agent --features http aether -- --nocapture`
  - 3 Aether route tests passed.
- `cargo check --workspace --all-features`
- `cargo fmt --all --check`
- `scripts/check-layering.sh`
  - Reported: `No layer-direction violations found.`
- `cargo clippy --all -- -D warnings`

## Self-Review

- `git diff --check` reported no whitespace errors.
- A targeted symbol search found no `AetherClient`, outbound response types,
  registry paths, or outbound event methods under `avs-agent` or
  `wiki/http-sidecar.md`.
- The inbound handler, both aliases, and the existing OpenAPI endpoint remain
  in place. No unrelated working-tree changes were present.

## Concerns

None. Future outbound Aether integration should be introduced only with a
sidecar owner and dedicated behavior tests.
