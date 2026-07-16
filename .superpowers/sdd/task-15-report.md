# Task 15 Report

## Scope

Audited and updated the ten Task 15 wiki pages against the implementation and
follow-up commits on `codex/pr31-framework-gaps`. Removed the duplicate final
Task 14 line from `.superpowers/sdd/progress.md`.

## Documentation Updates

- `wiki/hitl.md`, `wiki/skill.md`: trusted system-skill policy assembly and
  the deprecated compatibility guard, anchored to `ad73450`, `68140ed`,
  `dfb634b`, and `e313f37`.
- `wiki/strategy.md`: request-side native definitions and bounded dynamic
  routing, anchored to `9b3e717`, `c06a94f`, `75cc734`, `9cbb02f`, and
  `92fbf2a`. Native response-side tool parsing remains explicitly deferred.
- `wiki/guardrails.md`: the supported HITL runtime path and `ActionGuard`
  deprecation, anchored to `dfb634b`.
- `wiki/mcp.md`: removal of the pre-rewrite adapter and the maintained
  example-backed `McpLoader` path, anchored to `603c612`, `baf68ff`, and
  `1ee517c`. First-text-block response modeling remains intentional debt.
- `wiki/observability.md`: removal of legacy tracing scaffolding, anchored to
  `54baf3c`.
- `wiki/http-sidecar.md`: removal of the outbound Aether client while
  preserving inbound routes, anchored to `6420a3f`.
- `wiki/integration.md`: example-backed incubator ownership and dependency
  cleanup, anchored to `21bbea5` and `9f8049d`.
- `wiki/agent.md`, `wiki/subagent.md`: composition-root follow-ups and
  atomic first-registration-wins SubAgent wiring, anchored through `913f419`.

## Verification

- `./scripts/check-wiki.sh`
  - Exit 0: `check-wiki: OK (15 page(s))`
- `rg -n "Known debt|known gap|dead code|unwired|unused" wiki`
  - Exit 0. Matches remain only for still-real debt in `wiki/tools.md` and
    `wiki/eval-and-test-infra.md`, historical descriptions in
    `wiki/hitl.md`, `wiki/strategy.md`, and `wiki/memory.md`, and explicit
    statements of resolved removals/current forward-compatible fields in the
    audited pages. No stale `known gap` or `unwired` claim remains.
- `git diff --check`
  - Exit 0 with no output.

This documentation-only task did not manufacture a failing test.
