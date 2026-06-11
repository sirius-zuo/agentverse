# Staged Skill Workflow Examples Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two new examples — `doc-pipeline` (Pattern A: self-directing skill chain) and `support-router` (Pattern C: coordinator dispatch) — that together demonstrate all three AgentVerse strategies (ReAct, Plan, Hierarchical) in their natural contexts.

**Architecture:**
`doc-pipeline` runs a document through three skills sequentially, where each non-terminal skill emits `NEXT_SKILL: <name>` on its last line — the chain topology lives in the skills, not in `main.rs`. `support-router` uses a coordinator agent (ReAct, no tools) whose only job is outputting a JSON routing plan; `main.rs` then dispatches each step to the appropriate specialist agent (Hierarchical for billing, ReAct for the others).

**Strategy correction from spec:** The spec described the coordinator as using `StrategyKind::Plan`. After reading `avs-plan/src/plan.rs`, `PlanStrategy` both generates a plan AND executes it using registered tools — it cannot output a plan for external execution. The coordinator should use `StrategyKind::React` with no tools registered (React with zero tools = one-shot LLM call that returns JSON). `PlanStrategy` IS demonstrated in `doc-pipeline`'s `analyzer` stage, where it plans which entities to count, calls `count_mentions` per step, and synthesizes.

**Tech Stack:** Rust, `agentverse-agent`, `agentverse-strategy` (React/Plan/Hierarchical), `agentverse-demo-tools` (7 new mock tools), `agentverse-skill` (explicit binding via `create_session_with_skill`), `agentverse-session` (SQLite in-memory), `regex` (workspace dep, used by `find_dates`).

---

## File Map

**New files:**
- `examples/demo-tools/src/find_dates.rs`
- `examples/demo-tools/src/count_mentions.rs`
- `examples/demo-tools/src/word_count.rs`
- `examples/demo-tools/src/lookup_invoice.rs`
- `examples/demo-tools/src/check_refund_eligibility.rs`
- `examples/demo-tools/src/check_service_status.rs`
- `examples/demo-tools/src/get_account_details.rs`
- `examples/doc-pipeline/Cargo.toml`
- `examples/doc-pipeline/src/main.rs`
- `examples/doc-pipeline/skills/system/extractor/SKILL.md`
- `examples/doc-pipeline/skills/system/analyzer/SKILL.md`
- `examples/doc-pipeline/skills/system/summarizer/SKILL.md`
- `examples/support-router/Cargo.toml`
- `examples/support-router/src/main.rs`
- `examples/support-router/skills/system/coordinator/SKILL.md`
- `examples/support-router/skills/system/billing/SKILL.md`
- `examples/support-router/skills/system/tech-support/SKILL.md`
- `examples/support-router/skills/system/account-mgmt/SKILL.md`

**Modified files:**
- `examples/demo-tools/Cargo.toml` (add `regex`)
- `examples/demo-tools/src/lib.rs` (add 7 modules + pub use)
- `Cargo.toml` (add 2 workspace members)
- `README.md` (add both examples to tables)
- `DEVELOPMENT.md` (add both examples to directory listing)

---

## Task 1: Add `find_dates` tool

**Files:**
- Modify: `examples/demo-tools/Cargo.toml`
- Create: `examples/demo-tools/src/find_dates.rs`

- [ ] **Step 1: Add `regex` to demo-tools dependencies**

Open `examples/demo-tools/Cargo.toml` and add `regex` to the `[dependencies]` block:

```toml
[package]
name = "agentverse-demo-tools"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
agentverse    = { path = "../../avs-core" }
async-trait   = { workspace = true }
chrono        = { workspace = true }
regex         = { workspace = true }
schemars      = { workspace = true }
serde         = { workspace = true, features = ["derive"] }
serde_json    = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }
```

- [ ] **Step 2: Write the failing test**

Create `examples/demo-tools/src/find_dates.rs` with the tests only (no `Tool` impl yet):

```rust
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindDatesArgs {
    /// The text to search for date patterns
    pub text: String,
}

pub struct FindDates;

#[async_trait::async_trait]
impl Tool for FindDates {
    type Args = FindDatesArgs;
    fn name(&self) -> &str { "find_dates" }
    fn description(&self) -> &str { "todo" }
    async fn execute(&self, _args: FindDatesArgs) -> ToolResult {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn find_dates_returns_iso_and_us_formats() {
        let tool = FindDates;
        let result = tool.execute(FindDatesArgs {
            text: "Meeting on 2024-03-15 rescheduled to 4/10/2024.".to_string(),
        }).await.unwrap();
        let dates: Vec<serde_json::Value> = result.as_array().unwrap().to_vec();
        assert_eq!(dates.len(), 2);
        assert!(dates.iter().any(|d| d == "2024-03-15"));
        assert!(dates.iter().any(|d| d == "4/10/2024"));
    }

    #[tokio::test]
    async fn find_dates_returns_empty_when_no_dates() {
        let tool = FindDates;
        let result = tool.execute(FindDatesArgs {
            text: "No dates in this text at all.".to_string(),
        }).await.unwrap();
        assert!(result.as_array().unwrap().is_empty());
    }
}
```

- [ ] **Step 3: Run tests to confirm they fail**

```bash
cargo test -p agentverse-demo-tools find_dates 2>&1 | tail -5
```

Expected: compile error or panic at `unimplemented!()`.

- [ ] **Step 4: Implement `FindDates`**

Replace the stub `execute` in `find_dates.rs` with the real implementation:

```rust
use agentverse::{Tool, ToolResult};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindDatesArgs {
    /// The text to search for date patterns
    pub text: String,
}

pub struct FindDates;

#[async_trait::async_trait]
impl Tool for FindDates {
    type Args = FindDatesArgs;
    fn name(&self) -> &str { "find_dates" }
    fn description(&self) -> &str {
        "Find date patterns (YYYY-MM-DD and M/D/YYYY) in a text string. \
         Returns a JSON array of matched date strings."
    }
    async fn execute(&self, args: FindDatesArgs) -> ToolResult {
        let re = Regex::new(r"\b(\d{4}-\d{2}-\d{2}|\d{1,2}/\d{1,2}/\d{4})\b").unwrap();
        let dates: Vec<&str> = re.find_iter(&args.text).map(|m| m.as_str()).collect();
        Ok(json!(dates))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn find_dates_returns_iso_and_us_formats() {
        let tool = FindDates;
        let result = tool.execute(FindDatesArgs {
            text: "Meeting on 2024-03-15 rescheduled to 4/10/2024.".to_string(),
        }).await.unwrap();
        let dates: Vec<serde_json::Value> = result.as_array().unwrap().to_vec();
        assert_eq!(dates.len(), 2);
        assert!(dates.iter().any(|d| d == "2024-03-15"));
        assert!(dates.iter().any(|d| d == "4/10/2024"));
    }

    #[tokio::test]
    async fn find_dates_returns_empty_when_no_dates() {
        let tool = FindDates;
        let result = tool.execute(FindDatesArgs {
            text: "No dates in this text at all.".to_string(),
        }).await.unwrap();
        assert!(result.as_array().unwrap().is_empty());
    }
}
```

- [ ] **Step 5: Run tests to confirm they pass**

```bash
cargo test -p agentverse-demo-tools find_dates 2>&1 | tail -5
```

Expected: `test find_dates::tests::find_dates_returns_iso_and_us_formats ... ok` and `test find_dates::tests::find_dates_returns_empty_when_no_dates ... ok`.

---

## Task 2: Add `count_mentions` tool

**Files:**
- Create: `examples/demo-tools/src/count_mentions.rs`

- [ ] **Step 1: Write failing test**

Create `examples/demo-tools/src/count_mentions.rs`:

```rust
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CountMentionsArgs {
    /// The term to count (case-insensitive substring match per whitespace token)
    pub term: String,
    /// The text to search
    pub text: String,
}

pub struct CountMentions;

#[async_trait::async_trait]
impl Tool for CountMentions {
    type Args = CountMentionsArgs;
    fn name(&self) -> &str { "count_mentions" }
    fn description(&self) -> &str {
        "Count how many whitespace-separated tokens in text contain the term \
         (case-insensitive). Returns an integer."
    }
    async fn execute(&self, args: CountMentionsArgs) -> ToolResult {
        let needle = args.term.to_lowercase();
        let count = args.text
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.contains(needle.as_str()))
            .count() as u64;
        Ok(json!(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn count_mentions_is_case_insensitive() {
        let tool = CountMentions;
        let result = tool.execute(CountMentionsArgs {
            term: "rust".to_string(),
            text: "Rust is great. I love rust. RUST is fast.".to_string(),
        }).await.unwrap();
        assert_eq!(result, json!(3));
    }

    #[tokio::test]
    async fn count_mentions_returns_zero_for_no_match() {
        let tool = CountMentions;
        let result = tool.execute(CountMentionsArgs {
            term: "python".to_string(),
            text: "Rust is great. I love rust.".to_string(),
        }).await.unwrap();
        assert_eq!(result, json!(0));
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p agentverse-demo-tools count_mentions 2>&1 | tail -5
```

Expected: both tests pass (the implementation is already complete above).

---

## Task 3: Add `word_count` tool

**Files:**
- Create: `examples/demo-tools/src/word_count.rs`

- [ ] **Step 1: Create `word_count.rs`**

```rust
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WordCountArgs {
    /// The text to count words in
    pub text: String,
}

pub struct WordCount;

#[async_trait::async_trait]
impl Tool for WordCount {
    type Args = WordCountArgs;
    fn name(&self) -> &str { "word_count" }
    fn description(&self) -> &str {
        "Count the number of whitespace-separated words in a text string. Returns an integer."
    }
    async fn execute(&self, args: WordCountArgs) -> ToolResult {
        let count = args.text.split_whitespace().count() as u64;
        Ok(json!(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn word_count_counts_tokens() {
        let tool = WordCount;
        let result = tool.execute(WordCountArgs {
            text: "one two three four five".to_string(),
        }).await.unwrap();
        assert_eq!(result, json!(5));
    }

    #[tokio::test]
    async fn word_count_empty_string_returns_zero() {
        let tool = WordCount;
        let result = tool.execute(WordCountArgs {
            text: String::new(),
        }).await.unwrap();
        assert_eq!(result, json!(0));
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p agentverse-demo-tools word_count 2>&1 | tail -5
```

Expected: both tests pass.

---

## Task 4: Add `lookup_invoice` tool

**Files:**
- Create: `examples/demo-tools/src/lookup_invoice.rs`

- [ ] **Step 1: Create `lookup_invoice.rs`**

```rust
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LookupInvoiceArgs {
    /// Invoice ID to look up (e.g. "1042")
    pub invoice_id: String,
}

pub struct LookupInvoice;

#[async_trait::async_trait]
impl Tool for LookupInvoice {
    type Args = LookupInvoiceArgs;
    fn name(&self) -> &str { "lookup_invoice" }
    fn description(&self) -> &str {
        "Look up an invoice by ID. Returns invoice amount, date, status, and plan."
    }
    async fn execute(&self, args: LookupInvoiceArgs) -> ToolResult {
        Ok(json!({
            "invoice_id":   args.invoice_id,
            "amount_usd":   99.00,
            "date":         "2026-06-01",
            "status":       "paid",
            "plan":         "Pro",
            "description":  "Monthly subscription — Pro plan"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_invoice_returns_paid_invoice() {
        let tool = LookupInvoice;
        let result = tool.execute(LookupInvoiceArgs {
            invoice_id: "1042".to_string(),
        }).await.unwrap();
        assert_eq!(result["status"], "paid");
        assert_eq!(result["amount_usd"], 99.00);
        assert_eq!(result["invoice_id"], "1042");
    }
}
```

- [ ] **Step 2: Run test**

```bash
cargo test -p agentverse-demo-tools lookup_invoice 2>&1 | tail -5
```

Expected: test passes.

---

## Task 5: Add `check_refund_eligibility` tool

**Files:**
- Create: `examples/demo-tools/src/check_refund_eligibility.rs`

- [ ] **Step 1: Create `check_refund_eligibility.rs`**

```rust
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckRefundEligibilityArgs {
    /// Invoice ID to check eligibility for
    pub invoice_id: String,
}

pub struct CheckRefundEligibility;

#[async_trait::async_trait]
impl Tool for CheckRefundEligibility {
    type Args = CheckRefundEligibilityArgs;
    fn name(&self) -> &str { "check_refund_eligibility" }
    fn description(&self) -> &str {
        "Check whether a paid invoice is eligible for a refund. \
         Returns eligibility status, reason, and refund amount."
    }
    async fn execute(&self, args: CheckRefundEligibilityArgs) -> ToolResult {
        Ok(json!({
            "invoice_id":        args.invoice_id,
            "eligible":          true,
            "reason":            "Invoice is within the 30-day refund window",
            "refund_amount_usd": 99.00
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn check_refund_eligibility_returns_eligible() {
        let tool = CheckRefundEligibility;
        let result = tool.execute(CheckRefundEligibilityArgs {
            invoice_id: "1042".to_string(),
        }).await.unwrap();
        assert_eq!(result["eligible"], true);
        assert!(result["refund_amount_usd"].as_f64().unwrap() > 0.0);
    }
}
```

- [ ] **Step 2: Run test**

```bash
cargo test -p agentverse-demo-tools check_refund_eligibility 2>&1 | tail -5
```

Expected: test passes.

---

## Task 6: Add `check_service_status` tool

**Files:**
- Create: `examples/demo-tools/src/check_service_status.rs`

- [ ] **Step 1: Create `check_service_status.rs`**

```rust
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckServiceStatusArgs {
    /// Service or region to check (e.g. "api", "us-east-1")
    pub service: String,
}

pub struct CheckServiceStatus;

#[async_trait::async_trait]
impl Tool for CheckServiceStatus {
    type Args = CheckServiceStatusArgs;
    fn name(&self) -> &str { "check_service_status" }
    fn description(&self) -> &str {
        "Check the current operational status of a service or region. \
         Returns status (operational/degraded/outage) and incident details."
    }
    async fn execute(&self, args: CheckServiceStatusArgs) -> ToolResult {
        Ok(json!({
            "service":              args.service,
            "status":               "degraded",
            "region":               "us-east-1",
            "message":              "Elevated API latency observed since 2026-06-11T08:30:00Z",
            "estimated_resolution": "2026-06-11T14:00:00Z"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn check_service_status_returns_degraded() {
        let tool = CheckServiceStatus;
        let result = tool.execute(CheckServiceStatusArgs {
            service: "api".to_string(),
        }).await.unwrap();
        assert_eq!(result["status"], "degraded");
        assert_eq!(result["service"], "api");
    }
}
```

- [ ] **Step 2: Run test**

```bash
cargo test -p agentverse-demo-tools check_service_status 2>&1 | tail -5
```

Expected: test passes.

---

## Task 7: Add `get_account_details` tool

**Files:**
- Create: `examples/demo-tools/src/get_account_details.rs`

- [ ] **Step 1: Create `get_account_details.rs`**

```rust
use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAccountDetailsArgs {
    /// Account identifier or user email
    pub account_id: String,
}

pub struct GetAccountDetails;

#[async_trait::async_trait]
impl Tool for GetAccountDetails {
    type Args = GetAccountDetailsArgs;
    fn name(&self) -> &str { "get_account_details" }
    fn description(&self) -> &str {
        "Retrieve account details. Returns plan, seats, billing cycle, and renewal date."
    }
    async fn execute(&self, args: GetAccountDetailsArgs) -> ToolResult {
        Ok(json!({
            "account_id":     args.account_id,
            "plan":           "Pro",
            "seats":          5,
            "billing_cycle":  "monthly",
            "renewal_date":   "2026-12-01",
            "status":         "active"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_account_details_returns_pro_plan() {
        let tool = GetAccountDetails;
        let result = tool.execute(GetAccountDetailsArgs {
            account_id: "user@example.com".to_string(),
        }).await.unwrap();
        assert_eq!(result["plan"], "Pro");
        assert_eq!(result["status"], "active");
        assert_eq!(result["seats"], 5);
    }
}
```

- [ ] **Step 2: Run test**

```bash
cargo test -p agentverse-demo-tools get_account_details 2>&1 | tail -5
```

Expected: test passes.

---

## Task 8: Update `demo-tools/src/lib.rs` to export all 7 new tools

**Files:**
- Modify: `examples/demo-tools/src/lib.rs`

- [ ] **Step 1: Replace lib.rs with the updated version**

```rust
pub mod check_refund_eligibility;
pub mod check_service_status;
pub mod count_mentions;
pub mod find_dates;
pub mod get_account_details;
pub mod lookup_invoice;
pub mod market_sizing_calculator;
pub mod milestone_scheduler;
pub mod npv_calculator;
pub mod project_cost_estimator;
pub mod risk_adjusted_schedule;
pub mod runway_projector;
pub mod word_count;

pub use check_refund_eligibility::CheckRefundEligibility;
pub use check_service_status::CheckServiceStatus;
pub use count_mentions::CountMentions;
pub use find_dates::FindDates;
pub use get_account_details::GetAccountDetails;
pub use lookup_invoice::LookupInvoice;
pub use market_sizing_calculator::MarketSizingCalculator;
pub use milestone_scheduler::MilestoneScheduler;
pub use npv_calculator::NpvCalculator;
pub use project_cost_estimator::ProjectCostEstimator;
pub use risk_adjusted_schedule::RiskAdjustedSchedule;
pub use runway_projector::RunwayProjector;
pub use word_count::WordCount;

pub(crate) fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
```

- [ ] **Step 2: Verify the full test suite still passes**

```bash
cargo test -p agentverse-demo-tools 2>&1 | tail -10
```

Expected: all existing tests pass, plus the 11 new tests.

- [ ] **Step 3: Commit**

```bash
git add examples/demo-tools/
git commit -m "feat(demo-tools): add 7 mock tools for doc-pipeline and support-router examples"
```

---

## Task 9: Create `doc-pipeline` skills

**Files:**
- Create: `examples/doc-pipeline/skills/system/extractor/SKILL.md`
- Create: `examples/doc-pipeline/skills/system/analyzer/SKILL.md`
- Create: `examples/doc-pipeline/skills/system/summarizer/SKILL.md`

- [ ] **Step 1: Create extractor skill**

`examples/doc-pipeline/skills/system/extractor/SKILL.md`:

```markdown
---
name: extractor
description: >
  Extracts key facts, named entities, and dates from raw text.
  First stage of the document pipeline — declares NEXT_SKILL: analyzer.
version: 1.0.0
agentverse:
  tools:
    - find_dates
---

# Document Extractor

You extract structured information from raw text documents. You are the first stage
of a three-stage pipeline. When you finish, you pass your output to the analyzer stage.

## Workflow

1. Call `find_dates` on the full input text to locate all date patterns.
2. Read the text carefully and identify:
   - **Key Facts**: the main events, claims, or actions described
   - **Named Entities**: people, organizations, places, products
   - **Dates and Timeline**: use the `find_dates` results to place dates in context

## Output format

**Key Facts:**
- [fact 1]
- [fact 2]

**Named Entities:**
- [entity 1 — type]
- [entity 2 — type]

**Dates and Timeline:**
- [date]: [event]

On the very last line of your response, output exactly this — no trailing text:
NEXT_SKILL: analyzer
```

- [ ] **Step 2: Create analyzer skill**

`examples/doc-pipeline/skills/system/analyzer/SKILL.md`:

```markdown
---
name: analyzer
description: >
  Analyzes extracted facts and entities to find patterns and significance.
  Middle stage of the document pipeline — declares NEXT_SKILL: summarizer.
version: 1.0.0
agentverse:
  tools:
    - count_mentions
---

# Document Analyzer

You receive structured extraction output (facts, entities, dates) and analyze it for
patterns, relationships, and significance. You are the second stage of a three-stage
pipeline and use the Plan strategy — you plan your analysis steps before executing them.

## Workflow

Use the `count_mentions` tool to count how often key entities appear. When generating
your analysis plan, each step that calls `count_mentions` must pass the full input text
you received as the `text` argument.

Then identify:
- Which entities are most central (high mention frequency)
- What relationships exist between entities
- What patterns emerge across the timeline
- Any notable gaps or inconsistencies in the extracted data

## Output format

**Central Entities (by mention frequency):**
- [entity]: [N] mentions — [significance]

**Key Patterns:**
- [pattern]

**Notable Observations:**
- [observation]

On the very last line of your response, output exactly this — no trailing text:
NEXT_SKILL: summarizer
```

- [ ] **Step 3: Create summarizer skill**

`examples/doc-pipeline/skills/system/summarizer/SKILL.md`:

```markdown
---
name: summarizer
description: >
  Writes a concise executive summary from analyzed document content.
  Final stage of the document pipeline — no NEXT_SKILL directive.
version: 1.0.0
agentverse:
  tools:
    - word_count
---

# Document Summarizer

You write a concise executive summary from the analysis you receive. You are the final
stage of the pipeline.

## Workflow

1. Draft a 2-3 sentence executive overview followed by 3-5 bullet point key takeaways.
2. Call `word_count` on your draft.
3. If the count exceeds 150, shorten the draft and check again.
4. Output the final version once it is 150 words or fewer.

## Output format

**Executive Summary**

[2-3 sentence overview]

**Key Takeaways**
- [takeaway 1]
- [takeaway 2]
- [takeaway 3]

Do NOT add a NEXT_SKILL directive. This is the final stage — your output is printed directly.
```

- [ ] **Step 4: Commit skills**

```bash
git add examples/doc-pipeline/skills/
git commit -m "feat(doc-pipeline): add extractor, analyzer, summarizer skills"
```

---

## Task 10: Create `doc-pipeline` Cargo.toml and register in workspace

**Files:**
- Create: `examples/doc-pipeline/Cargo.toml`
- Modify: `Cargo.toml`

- [ ] **Step 1: Create `examples/doc-pipeline/Cargo.toml`**

```toml
[package]
name = "example-doc-pipeline"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
agentverse            = { path = "../../avs-core" }
agentverse-agent      = { path = "../../avs-agent" }
agentverse-demo-tools = { path = "../demo-tools" }
agentverse-logging    = { path = "../../avs-logging" }
agentverse-session    = { path = "../../avs-session" }
agentverse-strategy   = { path = "../../avs-strategy" }
agentverse-tools      = { path = "../../avs-tools" }
tokio                 = { workspace = true }
```

- [ ] **Step 2: Add to workspace `Cargo.toml`**

In the root `Cargo.toml`, add `"examples/doc-pipeline"` and `"examples/support-router"` to the `members` array (add both at once to avoid a second edit later):

```toml
[workspace]
members = [
    "avs-core",
    "avs-skill",
    "avs-agent",
    "avs-guardrails",
    "avs-integration",
    "avs-logging",
    "avs-memory",
    "avs-memory-lancedb",
    "avs-memory-pgvector",
    "avs-react",
    "avs-plan",
    "avs-router",
    "avs-tools",
    "avs-mcp",
    "avs-strategy",
    "examples/hello-agent",
    "examples/slack-hr-assistant",
    "examples/react-calculator",
    "examples/web-search-agent",
    "examples/code-review-agent",
    "examples/anthropic-react",
    "examples/http-agent",
    "examples/mcp-demo",
    "avs-session",
    "avs-subagent",
    "examples/demo-tools",
    "examples/project-feasibility",
    "examples/business-report",
    "examples/doc-pipeline",
    "examples/support-router",
]
resolver = "2"
```

- [ ] **Step 3: Verify workspace resolves**

```bash
cargo check -p example-doc-pipeline 2>&1 | tail -5
```

Expected: error about missing `src/main.rs` (no source yet) — but workspace member resolves. If you see "no such workspace member" the path is wrong.

---

## Task 11: Create `doc-pipeline/src/main.rs`

**Files:**
- Create: `examples/doc-pipeline/src/main.rs`

- [ ] **Step 1: Write the failing `parse_next_skill` tests first**

Create `examples/doc-pipeline/src/main.rs` with only the test module and the `parse_next_skill` stub:

```rust
fn parse_next_skill(_output: &str) -> (Option<&str>, &str) {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strips_next_skill_from_last_line() {
        let out = "Some content.\nMore content.\nNEXT_SKILL: analyzer";
        let (next, body) = parse_next_skill(out);
        assert_eq!(next, Some("analyzer"));
        assert_eq!(body, "Some content.\nMore content.");
    }

    #[test]
    fn parse_returns_none_when_no_directive() {
        let out = "Final summary with no directive.";
        let (next, body) = parse_next_skill(out);
        assert_eq!(next, None);
        assert_eq!(body, "Final summary with no directive.");
    }

    #[test]
    fn parse_handles_trailing_whitespace_after_directive() {
        let out = "Content.\nNEXT_SKILL: summarizer  \n  ";
        let (next, _) = parse_next_skill(out);
        assert_eq!(next, Some("summarizer"));
    }

    #[test]
    fn parse_handles_single_line_output_with_directive() {
        let out = "NEXT_SKILL: analyzer";
        let (next, body) = parse_next_skill(out);
        assert_eq!(next, Some("analyzer"));
        assert_eq!(body, "");
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test -p example-doc-pipeline 2>&1 | tail -5
```

Expected: panic at `unimplemented!()` or compile error.

- [ ] **Step 3: Write the complete `main.rs`**

Replace `examples/doc-pipeline/src/main.rs` with:

```rust
// examples/doc-pipeline/src/main.rs
//
// Pattern A — self-directing skill chain.
//
// Three skills form a sequential pipeline. Each non-terminal skill appends
// "NEXT_SKILL: <name>" as its final line, declaring its own successor.
// main.rs runs a loop that strips the directive, passes the clean output
// as input to the next stage, and stops when no directive is emitted.
// No stage names are hardcoded here — the chain lives in the skills.
//
// Strategies per stage:
//   extractor  → React  (calls find_dates to locate timeline events)
//   analyzer   → Plan   (plans which entities to count, calls count_mentions per step)
//   summarizer → React  (calls word_count to enforce a 150-word limit)
//
// Run:
//   ANTHROPIC_API_KEY=sk-ant-... \
//   MODEL_NAME=claude-sonnet-4-6 \
//   cargo run -p example-doc-pipeline -- "your document text here"

use agentverse::{Config, LlmRunner, PromptRegistry, ProviderConfig};
use agentverse_agent::{Agent, SkillConfig, SkillMode};
use agentverse_demo_tools::{CountMentions, FindDates, WordCount};
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::ToolRegistry;
use std::collections::HashSet;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    avs_logging::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} \"<document text>\"", args[0]);
        std::process::exit(1);
    }
    let input_doc = args[1..].join(" ");

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("MODEL_API_KEY"))
        .unwrap_or_default();
    let model_name = std::env::var("MODEL_NAME")
        .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::Anthropic {
                model_name: model_name.clone(),
                api_key,
            },
            max_messages: 50,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner"),
    );

    let tools = ToolRegistry::new();
    tools.register(FindDates);
    tools.register(CountMentions);
    tools.register(WordCount);

    let prompts = Arc::new(PromptRegistry::new());
    let skills_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/skills");

    // One agent per stage — each uses a different StrategyKind and is
    // constrained to its own skill so it cannot be routed elsewhere.
    let extractor_agent = make_agent(
        &runner, &tools, &prompts, StrategyKind::React,
        SkillMode::Constrained(vec!["extractor".into()]), skills_dir,
    ).await;
    let analyzer_agent = make_agent(
        &runner, &tools, &prompts, StrategyKind::Plan,
        SkillMode::Constrained(vec!["analyzer".into()]), skills_dir,
    ).await;
    let summarizer_agent = make_agent(
        &runner, &tools, &prompts, StrategyKind::React,
        SkillMode::Constrained(vec!["summarizer".into()]), skills_dir,
    ).await;

    let mut current_skill = "extractor".to_string();
    let mut input = input_doc;
    let mut seen: HashSet<String> = HashSet::new();

    loop {
        if !seen.insert(current_skill.clone()) {
            eprintln!("error: cycle detected — skill '{}' appeared twice", current_skill);
            std::process::exit(1);
        }

        let stage_agent = match current_skill.as_str() {
            "extractor"  => &extractor_agent,
            "analyzer"   => &analyzer_agent,
            "summarizer" => &summarizer_agent,
            other => {
                eprintln!("error: unknown skill '{}'", other);
                std::process::exit(1);
            }
        };

        println!("\n── stage: {} ──────────────────────────────", current_skill);

        let session_id = stage_agent
            .create_session_with_skill("user", &current_skill)
            .await
            .unwrap_or_else(|e| {
                eprintln!("error: create_session_with_skill failed: {e}");
                std::process::exit(1);
            });

        let output = stage_agent
            .invoke("user", session_id, &input)
            .await
            .unwrap_or_else(|e| {
                eprintln!("error: invoke failed: {e}");
                std::process::exit(1);
            });

        let (next_skill, clean_output) = parse_next_skill(&output);

        match next_skill {
            Some(next) => {
                input = clean_output.to_string();
                current_skill = next.to_string();
            }
            None => {
                println!("{}", clean_output);
                break;
            }
        }
    }
}

async fn make_agent(
    runner: &Arc<LlmRunner>,
    tools: &Arc<ToolRegistry>,
    prompts: &Arc<PromptRegistry>,
    strategy_kind: StrategyKind,
    mode: SkillMode,
    skills_dir: &str,
) -> Arc<Agent> {
    let strategy = build(
        strategy_kind,
        Arc::clone(runner),
        Arc::clone(prompts),
        Arc::clone(tools),
        10,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("session memory"),
    );
    let skills = SkillConfig::load(skills_dir, mode).expect("load skills");
    Agent::new(
        Arc::clone(runner),
        Arc::clone(tools),
        Arc::clone(prompts),
        session_memory,
        strategy,
        false,
        None,
        Some(skills),
    )
}

/// Strip `NEXT_SKILL: <name>` from the last non-empty line.
/// Returns `(Some(name), body_without_directive)` or `(None, full_output)`.
fn parse_next_skill(output: &str) -> (Option<&str>, &str) {
    let trimmed = output.trim_end();
    if let Some(last_newline) = trimmed.rfind('\n') {
        let last_line = trimmed[last_newline + 1..].trim();
        if let Some(rest) = last_line.strip_prefix("NEXT_SKILL:") {
            let skill_name = rest.trim();
            let body = trimmed[..last_newline].trim_end();
            return (Some(skill_name), body);
        }
    } else if let Some(rest) = trimmed.strip_prefix("NEXT_SKILL:") {
        return (Some(rest.trim()), "");
    }
    (None, trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strips_next_skill_from_last_line() {
        let out = "Some content.\nMore content.\nNEXT_SKILL: analyzer";
        let (next, body) = parse_next_skill(out);
        assert_eq!(next, Some("analyzer"));
        assert_eq!(body, "Some content.\nMore content.");
    }

    #[test]
    fn parse_returns_none_when_no_directive() {
        let out = "Final summary with no directive.";
        let (next, body) = parse_next_skill(out);
        assert_eq!(next, None);
        assert_eq!(body, "Final summary with no directive.");
    }

    #[test]
    fn parse_handles_trailing_whitespace_after_directive() {
        let out = "Content.\nNEXT_SKILL: summarizer  \n  ";
        let (next, _) = parse_next_skill(out);
        assert_eq!(next, Some("summarizer"));
    }

    #[test]
    fn parse_handles_single_line_output_with_directive() {
        let out = "NEXT_SKILL: analyzer";
        let (next, body) = parse_next_skill(out);
        assert_eq!(next, Some("analyzer"));
        assert_eq!(body, "");
    }
}
```

- [ ] **Step 4: Run the unit tests**

```bash
cargo test -p example-doc-pipeline 2>&1 | tail -10
```

Expected: all 4 `parse_*` tests pass. No LLM calls are made.

- [ ] **Step 5: Verify the crate compiles**

```bash
cargo build -p example-doc-pipeline 2>&1 | tail -5
```

Expected: `Compiling example-doc-pipeline` ... `Finished`.

- [ ] **Step 6: Commit**

```bash
git add examples/doc-pipeline/
git commit -m "feat(doc-pipeline): add self-directing skill chain example (ReAct + Plan + ReAct)"
```

---

## Task 12: Create `support-router` skills

**Files:**
- Create: `examples/support-router/skills/system/coordinator/SKILL.md`
- Create: `examples/support-router/skills/system/billing/SKILL.md`
- Create: `examples/support-router/skills/system/tech-support/SKILL.md`
- Create: `examples/support-router/skills/system/account-mgmt/SKILL.md`

- [ ] **Step 1: Create coordinator skill**

`examples/support-router/skills/system/coordinator/SKILL.md`:

```markdown
---
name: coordinator
description: >
  Reads a support request and produces a JSON routing plan assigning each
  issue to the appropriate specialist skill.
version: 1.0.0
agentverse:
  tools: []
---

# Support Coordinator

You read support requests and produce a routing plan for specialist agents.

## Available specialists

- **billing**: handles charges, invoices, payments, and refund requests
- **tech-support**: handles outages, errors, connectivity issues, and service status
- **account-mgmt**: handles upgrades, downgrades, cancellations, and profile changes

## Output

Output ONLY valid JSON — no prose, no explanation, no markdown fences.

The JSON must be an array of 1–3 objects. Each object has exactly two keys:
- `skill`: one of `billing`, `tech-support`, or `account-mgmt`
- `task`: a specific, self-contained instruction for that specialist

Example (do not include this example in your output):
[
  {"skill": "billing", "task": "Check whether the user was double-charged on invoice #1042 and determine if a refund is applicable"},
  {"skill": "tech-support", "task": "Check whether the API service in us-east-1 is currently degraded"}
]

Order the steps logically. Each distinct issue maps to exactly one specialist.
```

- [ ] **Step 2: Create billing skill**

`examples/support-router/skills/system/billing/SKILL.md`:

```markdown
---
name: billing
description: >
  Handles billing inquiries: charges, invoices, payments, and refund requests.
  Uses lookup_invoice and check_refund_eligibility tools.
version: 1.0.0
agentverse:
  tools:
    - lookup_invoice
    - check_refund_eligibility
---

# Billing Specialist

You handle billing and payment inquiries using a structured investigation approach.

## Workflow

1. Use `lookup_invoice` with the relevant invoice ID to retrieve invoice details.
2. If the customer is asking about a refund, use `check_refund_eligibility` to determine eligibility.
3. Provide a clear, specific answer: invoice status, refund eligibility, and next steps.

Do not guess — use the tools. Be precise about amounts, dates, and eligibility status.
```

- [ ] **Step 3: Create tech-support skill**

`examples/support-router/skills/system/tech-support/SKILL.md`:

```markdown
---
name: tech-support
description: >
  Handles technical issues: service outages, errors, connectivity, and status checks.
  Uses check_service_status tool.
version: 1.0.0
agentverse:
  tools:
    - check_service_status
---

# Technical Support Specialist

You handle technical issues and service status inquiries.

## Workflow

1. Use `check_service_status` with the relevant service name or region.
2. Based on the status, explain what is happening and provide actionable guidance.

Be specific about which services are affected, the estimated resolution time, and
what the user can do in the meantime (e.g., retry later, use a different region).
```

- [ ] **Step 4: Create account-mgmt skill**

`examples/support-router/skills/system/account-mgmt/SKILL.md`:

```markdown
---
name: account-mgmt
description: >
  Handles account management: plan changes, cancellations, and profile inquiries.
  Uses get_account_details tool.
version: 1.0.0
agentverse:
  tools:
    - get_account_details
---

# Account Management Specialist

You handle account changes and plan inquiries.

## Workflow

1. Use `get_account_details` with the user's account ID or email to retrieve account state.
2. Answer the user's question based on their current plan, seats, and renewal date.
3. Provide clear guidance on what plan changes are available and how to proceed.

Be specific about plan names, seat counts, billing cycles, and renewal dates.
```

- [ ] **Step 5: Commit skills**

```bash
git add examples/support-router/skills/
git commit -m "feat(support-router): add coordinator, billing, tech-support, account-mgmt skills"
```

---

## Task 13: Create `support-router` Cargo.toml

**Files:**
- Create: `examples/support-router/Cargo.toml`

(The workspace `Cargo.toml` was already updated in Task 10 Step 2.)

- [ ] **Step 1: Create `examples/support-router/Cargo.toml`**

```toml
[package]
name = "example-support-router"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
agentverse            = { path = "../../avs-core" }
agentverse-agent      = { path = "../../avs-agent" }
agentverse-demo-tools = { path = "../demo-tools" }
agentverse-logging    = { path = "../../avs-logging" }
agentverse-session    = { path = "../../avs-session" }
agentverse-strategy   = { path = "../../avs-strategy" }
agentverse-tools      = { path = "../../avs-tools" }
serde                 = { workspace = true, features = ["derive"] }
serde_json            = { workspace = true }
tokio                 = { workspace = true }
```

- [ ] **Step 2: Verify workspace sees both new crates**

```bash
cargo check -p example-support-router 2>&1 | tail -5
```

Expected: error about missing `src/main.rs` — that's fine at this step.

---

## Task 14: Create `support-router/src/main.rs`

**Files:**
- Create: `examples/support-router/src/main.rs`

- [ ] **Step 1: Write the failing `parse_plan` tests first**

Create `examples/support-router/src/main.rs` with the test module and a stub:

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PlanStep {
    skill: String,
    task: String,
}

fn parse_plan(_json: &str) -> Vec<PlanStep> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plan_deserializes_array() {
        let json = r#"[{"skill":"billing","task":"Check invoice"},{"skill":"tech-support","task":"Check status"}]"#;
        let steps = parse_plan(json);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].skill, "billing");
        assert_eq!(steps[1].skill, "tech-support");
    }

    #[test]
    fn parse_plan_strips_markdown_fences() {
        let json = "```json\n[{\"skill\":\"billing\",\"task\":\"Check\"}]\n```";
        let steps = parse_plan(json);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].skill, "billing");
    }

    #[test]
    fn parse_plan_handles_extra_prose_before_array() {
        let json = "Here is your plan:\n[{\"skill\":\"account-mgmt\",\"task\":\"Lookup\"}]";
        let steps = parse_plan(json);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].skill, "account-mgmt");
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test -p example-support-router 2>&1 | tail -5
```

Expected: panic at `unimplemented!()`.

- [ ] **Step 3: Write the complete `main.rs`**

Replace `examples/support-router/src/main.rs` with:

```rust
// examples/support-router/src/main.rs
//
// Pattern C — coordinator dispatch.
//
// A coordinator agent (ReAct, no tools) reads the support request and outputs
// a JSON plan: [{skill, task}, ...]. main.rs parses the plan and dispatches
// each step to the appropriate specialist agent, threading the previous step's
// output as context.
//
// Strategies per role:
//   coordinator  → React       (no tools; one-shot JSON output)
//   billing      → Hierarchical (decomposes: lookup invoice → check eligibility → draft)
//   tech-support → React       (single check_service_status call)
//   account-mgmt → React       (single get_account_details call)
//
// Run:
//   ANTHROPIC_API_KEY=sk-ant-... \
//   MODEL_NAME=claude-sonnet-4-6 \
//   cargo run -p example-support-router -- "I was charged twice last month and my API is down"

use agentverse::{Config, LlmRunner, PromptRegistry, ProviderConfig};
use agentverse_agent::{Agent, SkillConfig, SkillMode};
use agentverse_demo_tools::{
    CheckRefundEligibility, CheckServiceStatus, GetAccountDetails, LookupInvoice,
};
use agentverse_logging as avs_logging;
use agentverse_session::SqliteSessionMemory;
use agentverse_strategy::{build, StrategyKind};
use agentverse_tools::ToolRegistry;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct PlanStep {
    skill: String,
    task: String,
}

#[tokio::main]
async fn main() {
    avs_logging::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} \"<support request>\"", args[0]);
        std::process::exit(1);
    }
    let request = args[1..].join(" ");

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("MODEL_API_KEY"))
        .unwrap_or_default();
    let model_name = std::env::var("MODEL_NAME")
        .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: ProviderConfig::Anthropic {
                model_name: model_name.clone(),
                api_key,
            },
            max_messages: 50,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .expect("runner"),
    );

    let prompts = Arc::new(PromptRegistry::new());
    let skills_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/skills");

    // Coordinator: no tools — the skill instructs it to output only JSON.
    // React with zero tools = one-shot LLM call.
    let coordinator_tools = ToolRegistry::new();
    let coordinator_agent = make_agent(
        &runner,
        &coordinator_tools,
        &prompts,
        StrategyKind::React,
        SkillMode::Constrained(vec!["coordinator".into()]),
        skills_dir,
    )
    .await;

    // Specialists share the same tool registry; skills restrict which tools
    // are active per invocation via their `agentverse.tools` list.
    let specialist_tools = ToolRegistry::new();
    specialist_tools.register(LookupInvoice);
    specialist_tools.register(CheckRefundEligibility);
    specialist_tools.register(CheckServiceStatus);
    specialist_tools.register(GetAccountDetails);

    // billing uses Hierarchical: decomposes into sub-goals, each executed as a plan.
    // This demonstrates that a single dispatch step can itself be a multi-step chain.
    let billing_agent = make_agent(
        &runner,
        &specialist_tools,
        &prompts,
        StrategyKind::Hierarchical,
        SkillMode::Constrained(vec!["billing".into()]),
        skills_dir,
    )
    .await;

    let tech_support_agent = make_agent(
        &runner,
        &specialist_tools,
        &prompts,
        StrategyKind::React,
        SkillMode::Constrained(vec!["tech-support".into()]),
        skills_dir,
    )
    .await;

    let account_mgmt_agent = make_agent(
        &runner,
        &specialist_tools,
        &prompts,
        StrategyKind::React,
        SkillMode::Constrained(vec!["account-mgmt".into()]),
        skills_dir,
    )
    .await;

    // ── 1. Coordinator: produce routing plan ──────────────────────────────
    println!("\n── coordinator ─────────────────────────────────");
    let coord_session = coordinator_agent
        .create_session_with_skill("user", "coordinator")
        .await
        .expect("create coordinator session");

    let plan_json = coordinator_agent
        .invoke("user", coord_session, &request)
        .await
        .unwrap_or_else(|e| {
            eprintln!("coordinator error: {e}");
            std::process::exit(1);
        });

    println!("Plan: {}", plan_json.trim());

    let steps = parse_plan(&plan_json);

    // ── 2. Execute each step with the assigned specialist ─────────────────
    let mut context = String::new();

    for (i, step) in steps.iter().enumerate() {
        println!("\n── step {}: {} ─────────────────────────────────", i + 1, step.skill);

        let specialist = match step.skill.as_str() {
            "billing"      => &billing_agent,
            "tech-support" => &tech_support_agent,
            "account-mgmt" => &account_mgmt_agent,
            other => {
                eprintln!("error: unknown skill '{}' in coordinator plan", other);
                std::process::exit(1);
            }
        };

        let input = if context.is_empty() {
            step.task.clone()
        } else {
            format!("Task: {}\n\nContext from previous steps:\n{}", step.task, context)
        };

        let session_id = specialist
            .create_session_with_skill("user", &step.skill)
            .await
            .unwrap_or_else(|e| {
                eprintln!("error: create_session_with_skill '{}' failed: {e}", step.skill);
                std::process::exit(1);
            });

        context = specialist
            .invoke("user", session_id, &input)
            .await
            .unwrap_or_else(|e| {
                eprintln!("error: invoke '{}' failed: {e}", step.skill);
                std::process::exit(1);
            });

        println!("{}", context);
    }
}

async fn make_agent(
    runner: &Arc<LlmRunner>,
    tools: &Arc<ToolRegistry>,
    prompts: &Arc<PromptRegistry>,
    strategy_kind: StrategyKind,
    mode: SkillMode,
    skills_dir: &str,
) -> Arc<Agent> {
    let strategy = build(
        strategy_kind,
        Arc::clone(runner),
        Arc::clone(prompts),
        Arc::clone(tools),
        10,
    );
    let session_memory = Arc::new(
        SqliteSessionMemory::new("sqlite::memory:")
            .await
            .expect("session memory"),
    );
    let skills = SkillConfig::load(skills_dir, mode).expect("load skills");
    Agent::new(
        Arc::clone(runner),
        Arc::clone(tools),
        Arc::clone(prompts),
        session_memory,
        strategy,
        false,
        None,
        Some(skills),
    )
}

/// Parse the coordinator's JSON output into plan steps.
/// Strips markdown fences and finds the first `[...]` array.
fn parse_plan(json: &str) -> Vec<PlanStep> {
    let s = json.trim();
    let s = s.strip_prefix("```json").unwrap_or(s);
    let s = s.strip_prefix("```").unwrap_or(s);
    let s = s.strip_suffix("```").unwrap_or(s);
    let s = s.trim();
    let start = s.find('[').unwrap_or(0);
    let end = s.rfind(']').map(|i| i + 1).unwrap_or(s.len());
    let slice = &s[start..end];
    serde_json::from_str(slice).unwrap_or_else(|e| {
        eprintln!("error: failed to parse coordinator plan: {e}\nraw:\n{json}");
        std::process::exit(1);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plan_deserializes_array() {
        let json = r#"[{"skill":"billing","task":"Check invoice"},{"skill":"tech-support","task":"Check status"}]"#;
        let steps = parse_plan(json);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].skill, "billing");
        assert_eq!(steps[1].skill, "tech-support");
    }

    #[test]
    fn parse_plan_strips_markdown_fences() {
        let json = "```json\n[{\"skill\":\"billing\",\"task\":\"Check\"}]\n```";
        let steps = parse_plan(json);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].skill, "billing");
    }

    #[test]
    fn parse_plan_handles_extra_prose_before_array() {
        let json = "Here is your plan:\n[{\"skill\":\"account-mgmt\",\"task\":\"Lookup\"}]";
        let steps = parse_plan(json);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].skill, "account-mgmt");
    }
}
```

- [ ] **Step 4: Run unit tests**

```bash
cargo test -p example-support-router 2>&1 | tail -10
```

Expected: all 3 `parse_plan_*` tests pass.

- [ ] **Step 5: Verify the crate compiles**

```bash
cargo build -p example-support-router 2>&1 | tail -5
```

Expected: `Compiling example-support-router` ... `Finished`.

- [ ] **Step 6: Commit**

```bash
git add examples/support-router/
git commit -m "feat(support-router): add coordinator-dispatch example (React + Hierarchical + React)"
```

---

## Task 15: Update README and DEVELOPMENT

**Files:**
- Modify: `README.md`
- Modify: `DEVELOPMENT.md`

- [ ] **Step 1: Add both examples to the README staged-skill-workflow section**

In `README.md`, find the "Multi-agent examples" table (around line 433–439) and add a new table for the staged skill workflow examples directly below it:

```markdown
Staged skill workflow examples (require `ANTHROPIC_API_KEY` + `MODEL_NAME`):

| Package | Pattern | Strategies | Concept demonstrated |
|---|---|---|---|
| `example-doc-pipeline` | A — self-directing chain | ReAct → Plan → ReAct | Skills declare their own successors via `NEXT_SKILL: <name>`; chain topology lives in skills, not in `main.rs` |
| `example-support-router` | C — coordinator dispatch | React (coordinator) + Hierarchical (billing) + React (specialists) | Coordinator emits a JSON routing plan; `main.rs` dispatches each step to the specialist agent with the matching skill |
```

- [ ] **Step 2: Add both examples to the DEVELOPMENT.md directory listing**

In `DEVELOPMENT.md`, find the `examples/` directory listing (around line 47–57) and add two lines inside the `└── examples/` block:

```
    ├── doc-pipeline/       # Pattern A: self-directing skill chain (extractor→analyzer→summarizer; ReAct+Plan+ReAct)
    └── support-router/     # Pattern C: coordinator dispatch (coordinator plans, specialists execute; React+Hierarchical+React)
```

- [ ] **Step 3: Run full workspace check to confirm nothing is broken**

```bash
cargo check --workspace 2>&1 | tail -10
```

Expected: no errors.

- [ ] **Step 4: Run full workspace test suite**

```bash
cargo test --workspace 2>&1 | tail -15
```

Expected: all tests pass (the new unit tests in demo-tools, doc-pipeline, and support-router are included).

- [ ] **Step 5: Commit**

```bash
git add README.md DEVELOPMENT.md
git commit -m "docs: add doc-pipeline and support-router to README and DEVELOPMENT"
```

---

## Self-Review

**Spec coverage check:**
- ✅ `doc-pipeline`: self-directing chain (Pattern A), three stages, ReAct + Plan + ReAct
- ✅ `support-router`: coordinator dispatch (Pattern C), React coordinator, Hierarchical billing, React for others
- ✅ All three mock tool families (find_dates/count_mentions/word_count, lookup_invoice/check_refund_eligibility/check_service_status/get_account_details)
- ✅ NEXT_SKILL directive parsed in doc-pipeline
- ✅ JSON plan parsed in support-router
- ✅ Context threading between support-router steps
- ✅ Cycle guard in doc-pipeline
- ✅ Fast-fail error handling in both examples
- ✅ README and DEVELOPMENT updated

**Strategy correction documented:** The plan header explains why `PlanStrategy` is used in `doc-pipeline/analyzer` rather than `support-router/coordinator` — this deviates from the approved spec but is architecturally correct given `PlanStrategy`'s plan-and-execute semantics.

**Type consistency:** `make_agent` signature is identical in both `main.rs` files. `parse_next_skill` in doc-pipeline and `parse_plan` in support-router are consistent with how they're called.

**No placeholders:** All SKILL.md files contain complete instructions. All tool implementations return concrete values. All code blocks are complete.
