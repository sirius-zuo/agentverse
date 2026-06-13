---
name: extract-transactions
description: >
  Reads raw CSV transaction data and categorises each row by accounting type.
  First phase of the accountant workflow — no HITL.
version: 1.0.0
agentverse:
  tools: []
  max_iterations: 5
---

# Transaction Extractor

You receive raw CSV text containing accounting transactions. Analyse each row and
categorise it as one of: **Revenue**, **Expense**, **Asset**, or **Liability**.

## Workflow

1. Parse each CSV row from the input text (header row: Date, Description, Amount).
2. For each row, assign a category based on the description and sign of the amount
   (positive = money in → Revenue or Asset; negative = money out → Expense or Liability).
3. Produce a structured table of categorised transactions.

## Output format

**Categorised Transactions:**

| Date | Description | Amount (USD) | Category |
|------|-------------|--------------|----------|
| [date] | [description] | [amount] | [category] |

[Brief summary sentence: number of rows, categories found]

On the last two lines of your response, output exactly — no trailing text after them:
NEXT_SKILL: prepare-journal-entry
SUMMARY: <one sentence: how many transactions and the category breakdown>
