# Task 11 Report: Remove Dead `avs-core` Tracing Scaffolding

Base: `f1bb1de`

## Scope Completed

- Removed the `agentverse::tracing` module, its `Tracer`, `Span`,
  `NoopTracer`, `OtelTracer`, and `DefaultTracer` types, and the public
  re-exports from `avs-core/src/lib.rs`.
- Removed the default-on `tracing` Cargo feature and its optional
  `opentelemetry-otlp = 0.15` dependency from `avs-core/Cargo.toml`.
- Refreshed `Cargo.lock` using `cargo update -p opentelemetry-otlp@0.15.0`.
  Cargo removed the obsolete OTLP 0.15 dependency family; the current OTLP
  exporter dependency owned by `examples/http-agent` remains.
- Updated `wiki/observability.md` and `wiki/core-runtime.md` to name
  `avs-core::metrics` and `avs-logging` as the maintained observability paths
  and to remove the legacy-scaffolding debt description.

## Pre-Change Ownership Evidence

1. `cargo tree -p agentverse --features tracing` showed that the legacy
   feature pulled in `opentelemetry-otlp v0.15.0`, including its separate
   OpenTelemetry 0.22/tonic dependency family, in addition to the maintained
   OpenTelemetry 0.32 metrics API and SDK dependencies.
2. `cargo tree -i opentelemetry-otlp@0.15.0` identified `agentverse`
   (`avs-core`) as the direct and only root of that dependency. Its many
   workspace descendants consumed it only transitively through `agentverse`.
3. A workspace Cargo-manifest scan found all `agentverse = { path = ... }`
   dependencies without a feature list and found no `agentverse/tracing`
   enablement. The other `tracing.workspace = true` entries use the separate
   Rust logging crate and are owned by `avs-logging` and instrumented crates,
   not this removed feature.
4. A workspace source scan found `Tracer`, `NoopTracer`, `OtelTracer`, and
   `DefaultTracer` only in the dead module, its `lib.rs` re-export, and the
   documentation that described the debt. No code constructed a tracer or
   invoked a span.

## Test Strategy

This is dead API and dependency removal, not a behavior addition or bug fix.
A manufactured failing test would assert the obsolete public API only to be
deleted with it, so it would not protect a maintained behavior. Existing
metrics regression tests and the all-features workspace compile check instead
cover the retained observability surface and every workspace consumer.

## Verification

- `cargo test -p agentverse metrics -- --nocapture` passed outside the
  filesystem sandbox. The first sandbox attempt could not bind the loopback
  HTTP mock used by `metrics_llm_test`; the approved rerun passed its metric
  retry/body-read regression test.
- `cargo test -p agentverse --test metrics_facade_test -- --nocapture` passed:
  `facade_records_all_instruments_with_expected_names_and_attributes`.
- `cargo check --workspace --all-features` passed.
- `cargo fmt --all --check` passed.
- `git diff --check` passed.
- Post-change scans found no workspace manifest enabling `agentverse/tracing`
  and no remaining `Tracer`/`NoopTracer`/`OtelTracer`/`DefaultTracer` source
  references. `cargo tree -i opentelemetry-otlp@0.15.0` no longer resolves a
  package; only `opentelemetry-otlp@0.32.0` remains for the HTTP example.

## Deferred Gate

Stage 5 layering/clippy remains deferred until Task 12, as directed.
