# Task 13 Report: Correct the Stale HITL Queue Comment

## Status

Complete. The `ApprovalQueue::sweep_expired` doc comment now identifies
`HitlSweepWorker` as its caller.

## Scope Confirmation

- Updated `avs-hitl/src/queue.rs` only for the requested source documentation.
- Updated the directly anchored stale-name explanation in `wiki/hitl.md`.
- No runtime behavior or unrelated documentation changed.

## TDD Applicability

No TDD test is meaningful for this comment-only correction. The change does
not alter compiled behavior or a testable runtime contract.

## Verification

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.

## Concerns

None for the requested scope.

## Commit

This report is included in the Task 13 commit.
