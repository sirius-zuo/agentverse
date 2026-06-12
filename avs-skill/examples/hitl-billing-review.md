---
name: billing-review
description: |
  Multi-phase billing review workflow with human-in-the-loop gates.
  Reviews billing changes, drafts a summary, then waits for human
  approval before posting.
version: 0.1.0
tags:
  - billing
  - finance
  - hitl

agentverse:
  tools:
    - ledger_read
    - ledger_post
    - request_checkpoint
  max_iterations: 20
  activation:
    domains:
      - billing
      - finance
  # HITL configuration
  hitl_tools:
    - ledger_post  # Require approval before writing to ledger
  phase_gate: true  # Require approval before entering each phase
  checkpoints:
    - review_complete  # Draft review ready for human check
    - approved_to_post  # Approved to post changes
---

# Billing Review Skill

## Phase 1: Ledger Review

Read the relevant ledger entries and produce a structured summary of changes.

1. Use `ledger_read` to fetch entries for the specified period
2. Analyze discrepancies or unusual patterns
3. Write findings to the checkpoint: `request_checkpoint(review_complete, "Review findings: ...")`

**Phase Gate:** Human must approve before proceeding to Phase 2.

## Phase 2: Draft Summary

Generate a summary of findings for the stakeholder.

1. Review the checkpoint output from Phase 1
2. Draft a clear, actionable summary
3. Output the summary for Phase 2 gate

**Phase Gate:** Human must approve before proceeding to Phase 3.

## Phase 3: Post Changes

Apply approved changes to the ledger.

1. Only proceed if human has approved the summary from Phase 2
2. Use `ledger_post` to record the changes (HITL-gated)
3. Confirm posting was successful

> **HITL Note:** `ledger_post` is in the `hitl_tools` list, so execution pauses
> and waits for human approval before writing to the ledger.
