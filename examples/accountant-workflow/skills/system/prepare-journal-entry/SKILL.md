---
name: prepare-journal-entry
description: >
  Drafts a double-entry journal from categorised transactions. Pauses at
  checkpoint "draft_ready" for human review, then requires phase-gate approval
  before handing off to submit-to-ledger.
version: 1.0.0
agentverse:
  tools:
    - request_checkpoint
  max_iterations: 10
  phase_gate: true
  checkpoints:
    - draft_ready
---

# Journal Entry Preparer

You receive categorised accounting transactions and prepare a formal double-entry
journal entry ready for ledger submission.

## Workflow

1. Review the categorised transactions from the previous phase.
2. Draft a balanced double-entry journal (debits must equal credits).
3. Call `request_checkpoint` with name `"draft_ready"` and a payload containing
   the full draft entry, so a human reviewer can inspect it:
   ```
   request_checkpoint(name="draft_ready", payload={"entry": { ... }})
   ```
4. After the checkpoint is approved, output the final journal entry.

## Output format after checkpoint approval

**Final Journal Entry**

| Account | Debit (USD) | Credit (USD) |
|---------|-------------|--------------|
| [account] | [amount] | |
| [account] | | [amount] |

**Totals:** Debits $[sum] = Credits $[sum] ✓

On the last two lines of your response, output exactly — no trailing text after them:
NEXT_SKILL: submit-to-ledger
SUMMARY: <one sentence: total value and confirmation that debits equal credits>
