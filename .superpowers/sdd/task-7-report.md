# Task 7 Report: Wire StrategyRouter into AgentBuilder

## Scope

- Added `agentverse-router` as a Layer 4 `avs-agent` dependency and exposed
  `AgentBuilder::with_strategy_router(StrategyRouter)` as an explicit opt-in.
- Preserved the required fixed `Arc<dyn RunStrategy>` for agents without a
  router and for stateless invocation.
- Routed each session-aware invocation, converted `StrategyName` to
  `StrategyKind` in `avs-agent`, and constructed the selected strategy from
  the agent's runner, prompts, and tools with
  `DEFAULT_ROUTED_STRATEGY_MAX_ITERATIONS = 10`.
- Preserved active tool filtering and continued routed ReAct across HITL
  resume. Routed Plan/Hierarchical with configured HITL fail closed through a
  dedicated `AgentError` because those strategies do not intercept hooks.
- Updated `wiki/agent.md` and `wiki/strategy.md`. No dedicated router wiki
  page exists.

## RED Evidence

1. Added deterministic fake-HTTP routing tests before the production wiring.
   `cargo test -p agentverse-agent strategy_router -- --nocapture` failed with
   exit code 101 because `AgentBuilder::with_strategy_router` and
   `AgentError::RoutedStrategyDoesNotSupportHitl` did not exist.
2. Added `routed_react_strategy_is_preserved_across_hitl_resume` during
   self-review. Its first run failed with exit code 101 at the expected output
   assertion because `resume` used the supplied fixed strategy instead of the
   routed ReAct strategy.
3. The first Stage 3 clippy run failed with exit code 101 because the new
   `AgentError` variant exposed three non-exhaustive HTTP status matches. This
   identified the feature-enabled integration sites before commit.

## GREEN Evidence

1. `strategy_router_selects_plan_strategy_on_every_invoke` passed and records
   two router calls, two Plan generation calls, and two Plan synthesis calls;
   the Plan-only synthesis response proves the selected strategy executed.
2. `no_strategy_router_preserves_supplied_fixed_strategy` passed with exactly
   one call to the supplied strategy and no model request.
3. `strategy_router_fails_closed_when_plan_is_selected_with_hitl` passed with
   the dedicated error and zero fixed-strategy calls.
4. `routed_react_strategy_is_preserved_across_hitl_resume` passed after resume
   rebuilt ReAct under the routing/HITL invariant; the fixed strategy remained
   unused.

## Stage 3 Gates

- `cargo test -p agentverse-router -p agentverse-agent strategy -- --nocapture`
  passed: 8 selected tests, 0 failures.
- `cargo fmt --all --check` passed.
- `scripts/check-layering.sh` passed: no layer-direction violations.
- `cargo clippy --all -- -D warnings` passed across the workspace.
- `git diff --check` passed before report generation.

## Self-Review

The initial review found the routed ReAct resume defect described above and
the Stage 3 gate found the HTTP enum integration gap; both were fixed and
re-verified. The final review found no remaining correctness, layering,
security, or documentation issues in Task 7 scope.

## Concerns

None.

## Review Fixes

### RED Evidence

1. `routed_strategy_cannot_execute_tools_excluded_by_active_tool_names`
   initially failed with `Ok(Done("tool ran"))`: routed ReAct executed a real
   registered `echo` tool even though the bound skill declared `tools: []`.
2. `strategy_router_rejects_model_selection_outside_allowlist` initially
   failed because `StrategyRouter::route` returned `Ok(PlanAndExecute)` while
   configured with only `ReAct`.
3. Self-review added
   `routed_strategy_cannot_execute_inactive_tool_after_hitl_approval`; it
   initially failed with an execution count of 1 because resume dispatched an
   approved inactive call through the full agent registry.

### GREEN Evidence

1. Routed strategy construction now receives an exact-name restricted
   `ToolRegistry`; the empty-skill regression returns `ToolError::NotFound`
   and records zero tool executions.
2. `StrategyRouter::route` now rejects recognized model selections outside
   its configured available-strategy allowlist.
3. Routed HITL resume uses the persisted active names both for strategy
   reconstruction and approved/modified pending-call execution. The focused
   regression records zero inactive tool executions and one router request
   across invoke plus resume.
4. The existing routed ReAct resume regression now explicitly asserts exactly
   one router request across invoke plus resume.

### Review-Fix Gates

- `cargo test -p agentverse-router -p agentverse-agent strategy -- --nocapture`
  passed: 11 selected tests, 0 failures.
- `cargo fmt --all --check` passed.
- `scripts/check-layering.sh` passed: no layer-direction violations.
- `cargo clippy --all -- -D warnings` passed across the workspace.

### Review-Fix Self-Review

The review found and fixed the HITL approval continuation bypass in addition
to the requested invoke-time registry boundary. The final diff preserves the
specialized `ToolRegistry::filter_by_names` subagent-spawner exclusion while
adding a separate exact-name restriction API for top-level routed agents. No
remaining correctness, security, layering, or compatibility concerns were
found in the review-fix scope.
