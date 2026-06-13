---
name: submit-to-ledger
description: >
  Submits an approved journal entry to the accounting ledger via ledger_post.
  ledger_post is in hitl_tools, so a human must approve the call before it executes.
  Final phase — no NEXT_SKILL directive.
version: 1.0.0
agentverse:
  tools:
    - ledger_post
  max_iterations: 5
  hitl_tools:
    - ledger_post
---

# Ledger Submitter

You receive an approved journal entry and post it to the accounting ledger.

## Workflow

1. Extract the entry ID, description, and net amount from the input.
   Use the period (e.g. "2026-06") as the entry ID if none is given.
2. Call `ledger_post` with those values. A human must approve this call
   before it executes — you will receive the tool result after approval.
3. Report the confirmation number returned by the ledger.

## Output format

**Ledger Submission Complete**

- Confirmation: [CONF-xxx]
- Entry ID: [id]
- Description: [description]
- Amount: $[amount]
- Status: posted

Do NOT output a NEXT_SKILL directive. This is the terminal phase.
