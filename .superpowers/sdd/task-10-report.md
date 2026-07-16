# Task 10 Report: Explicit Integration Incubator

## Status

Complete. `agentverse-integration` is now documented as an example-backed
incubator rather than an `avs-agent` core runtime path.

## Changes

- Removed the unused normal and dev `agentverse` (`avs-core`) dependencies
  from `avs-integration/Cargo.toml`.
- Added crate-level documentation explaining that `IntegrationRuntime` is
  maintained by the integration tests and `example-slack-hr-assistant`, not
  as a core runtime integration.
- Deprecated `WhatsAppConnector` with the note `stub connector; not
  production-ready`, while preserving its constructor, trait implementations,
  and typed `ConnectorError::Connection` not-implemented errors.
- Scoped `allow(deprecated)` to the connector's internal implementation and
  configuration construction plus the test call sites. External consumers
  continue to receive the deprecation warning.
- Added assertions that both WhatsApp input and output operations return the
  explicit typed not-implemented error.
- Updated `wiki/integration.md` to label the crate as an incubator, remove
  the stale normal-core-dependency claim, and describe WhatsApp deprecation.
- Left `examples/slack-hr-assistant` unchanged.

## TDD Note

TDD red/green was not applicable to the dependency metadata, crate docs,
deprecation annotation, and wiki updates because they do not introduce a new
runtime behavior. The existing WhatsApp behavior was intentionally preserved;
focused tests were added to document its compatibility contract rather than
to drive a behavior change.

## Verification

All commands completed successfully from the Task 10 worktree:

- `cargo test -p agentverse-integration -- --nocapture` - 28 tests passed;
  0 failed; 0 ignored; doc-tests passed.
- `cargo check -p example-slack-hr-assistant` - passed.
- `cargo fmt --all --check` - passed.
- `scripts/check-layering.sh` - `No layer-direction violations found.`
- `cargo clippy --all -- -D warnings` - passed.
- `git diff --check 1ee517c -- avs-integration wiki/integration.md
  examples/slack-hr-assistant .superpowers/sdd/task-10-report.md` - passed.

## Self-Review

Reviewed the Task 10 diff against base `1ee517c`. No correctness, layering,
compatibility, formatting, or documentation issues found. The existing
unrelated modification to `.superpowers/sdd/task-9-report.md` was preserved
and excluded from this task's commit.

## Concerns

None. `WhatsAppConnector` remains intentionally non-production and is now
explicitly marked as such for consumers.

## Review Fix

Removed the also-unused `agentverse` dev-dependency and narrowed the WhatsApp
compatibility test allowances from the whole test target to the deprecated
import and four intentional compatibility tests. Removing that dev-dependency
also exposed an accidental Tokio feature-unification dependency, so
`avs-integration` now directly requests the `io-std` feature used by
`ConsoleConnector`.
