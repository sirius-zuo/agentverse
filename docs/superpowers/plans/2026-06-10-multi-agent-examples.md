# Multi-Agent Examples Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add three crates — `examples/demo-tools`, `examples/project-feasibility`, and `examples/business-report` — that together demonstrate programmatic and LLM-driven subagent orchestration over a shared set of six domain-specific MCP tools.

**Architecture:** `demo-tools` defines six pure-computation tools exposed via MCP. `project-feasibility` calls `SubAgentExecutor::run_many()` directly: three analyst subagents run in parallel, then a synthesis subagent reads their results as `ResourceContent`. `business-report` registers `SubAgentTool` into an Agent's tool registry and activates a `business-report` skill that instructs the LLM to spawn three analysts via `spawn_subagent`, then write a synthesis.

**Tech Stack:** Rust, tokio, `agentverse-subagent`, `agentverse-mcp`, `agentverse-agent`, `agentverse-tools`, `agentverse-strategy`, `agentverse-session`, OpenAI-compatible provider via `MODEL_BASE_URL` / `MODEL_API_KEY` / `MODEL_NAME` env vars.

---

## File Map

```
Cargo.toml                                          ← add 3 workspace members

examples/demo-tools/
  Cargo.toml                                        ← new crate
  src/
    lib.rs                                          ← pub re-exports for all 6 tools
    project_cost_estimator.rs                       ← ProjectCostEstimator + unit test
    npv_calculator.rs                               ← NpvCalculator + unit test
    milestone_scheduler.rs                          ← MilestoneScheduler + unit test
    risk_adjusted_schedule.rs                       ← RiskAdjustedSchedule + unit test
    market_sizing_calculator.rs                     ← MarketSizingCalculator + unit test
    runway_projector.rs                             ← RunwayProjector + unit test

examples/project-feasibility/
  Cargo.toml                                        ← new crate
  prompts/react.j2                                  ← format-only ReAct preamble
  src/main.rs                                       ← MCP server → run_many → synthesis

examples/business-report/
  Cargo.toml                                        ← new crate
  prompts/react.j2                                  ← format-only ReAct preamble (same template)
  prompts/system.j2                                 ← main agent persona
  skills/system/business-report/SKILL.md            ← skill: spawn 3 analysts + synthesize
  src/main.rs                                       ← MCP server → Agent + skill → invoke
```

---

## Task 1: Workspace registration + `demo-tools` scaffold

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `examples/demo-tools/Cargo.toml`
- Create: `examples/demo-tools/src/lib.rs`

- [ ] **Step 1: Add the three new crates to the workspace `members` list**

  In `Cargo.toml`, add after `"avs-subagent"`:

  ```toml
      "examples/demo-tools",
      "examples/project-feasibility",
      "examples/business-report",
  ```

- [ ] **Step 2: Create `examples/demo-tools/Cargo.toml`**

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
  schemars      = { workspace = true }
  serde         = { workspace = true, features = ["derive"] }
  serde_json    = { workspace = true }

  [dev-dependencies]
  tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }
  ```

- [ ] **Step 3: Create `examples/demo-tools/src/lib.rs` with module stubs**

  ```rust
  pub mod market_sizing_calculator;
  pub mod milestone_scheduler;
  pub mod npv_calculator;
  pub mod project_cost_estimator;
  pub mod risk_adjusted_schedule;
  pub mod runway_projector;

  pub use market_sizing_calculator::MarketSizingCalculator;
  pub use milestone_scheduler::MilestoneScheduler;
  pub use npv_calculator::NpvCalculator;
  pub use project_cost_estimator::ProjectCostEstimator;
  pub use risk_adjusted_schedule::RiskAdjustedSchedule;
  pub use runway_projector::RunwayProjector;
  ```

- [ ] **Step 4: Verify the workspace compiles with the new crate (no source files yet → expect missing module errors)**

  ```bash
  cargo check -p agentverse-demo-tools 2>&1 | head -20
  ```

  Expected: errors about missing files `project_cost_estimator.rs` etc. — this confirms the crate is wired in. (If workspace itself errors, check Cargo.toml path spelling.)

- [ ] **Step 5: Create empty source files so the crate compiles**

  Create each of the six module files with a single comment so the crate builds:

  ```bash
  for f in project_cost_estimator npv_calculator milestone_scheduler \
            risk_adjusted_schedule market_sizing_calculator runway_projector; do
    echo "// placeholder" > examples/demo-tools/src/${f}.rs
  done
  ```

- [ ] **Step 6: Verify the crate compiles cleanly**

  ```bash
  cargo check -p agentverse-demo-tools
  ```

  Expected: no errors.

- [ ] **Step 7: Commit**

  ```bash
  git add Cargo.toml examples/demo-tools/
  git commit -m "chore: add demo-tools, project-feasibility, business-report to workspace"
  ```

---

## Task 2: Financial tools — `ProjectCostEstimator` + `NpvCalculator`

**Files:**
- Modify: `examples/demo-tools/src/project_cost_estimator.rs`
- Modify: `examples/demo-tools/src/npv_calculator.rs`

### ProjectCostEstimator

Inputs: team size, average monthly salary, duration, overhead %, projected revenue for years 1 and 2.
Outputs: total cost, monthly burn rate, 24-month cumulative revenue, net profit/loss, ROI %.

Formulas:
- `monthly_burn = team_size × avg_monthly_salary × (1 + overhead_pct)`
- `total_cost = monthly_burn × duration_months`
- `net = revenue_yr1 + revenue_yr2 − total_cost`
- `roi_pct = (net / total_cost) × 100`

### NpvCalculator

Inputs: initial investment, annual cash flows (Vec), discount rate.
Outputs: NPV, approximate IRR (binary search), payback period in years, per-year cumulative table.

Formulas:
- `NPV = Σ(CF_t / (1+r)^t) − investment`
- IRR via bisection over `[−0.99, 10.0]` (50 iterations)
- Payback: first year cumulative ≥ 0

---

- [ ] **Step 1: Replace `project_cost_estimator.rs` with the stub that has `todo!()` in `execute`**

  ```rust
  use agentverse::{Tool, ToolResult};
  use schemars::JsonSchema;
  use serde::Deserialize;
  use serde_json::json;

  #[derive(Debug, Deserialize, JsonSchema)]
  pub struct ProjectCostArgs {
      /// Number of engineers on the team
      pub team_size: u32,
      /// Average monthly salary per engineer in USD
      pub avg_monthly_salary_usd: f64,
      /// Project duration in months
      pub duration_months: u32,
      /// Overhead as a decimal (e.g. 0.3 for 30%)
      pub overhead_pct: f64,
      /// Projected revenue in year 1 in USD
      pub projected_revenue_year1_usd: f64,
      /// Projected revenue in year 2 in USD
      pub projected_revenue_year2_usd: f64,
  }

  pub struct ProjectCostEstimator;

  #[async_trait::async_trait]
  impl Tool for ProjectCostEstimator {
      type Args = ProjectCostArgs;
      fn name(&self) -> &str { "project_cost_estimator" }
      fn description(&self) -> &str {
          "Estimate software project development cost and ROI. Calculates total cost, \
           monthly burn rate, and 2-year ROI given team size, salaries, duration, \
           overhead, and projected revenues."
      }
      async fn execute(&self, _args: ProjectCostArgs) -> ToolResult {
          todo!()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn project_cost_estimator_computes_correctly() {
          let tool = ProjectCostEstimator;
          let args = ProjectCostArgs {
              team_size: 4,
              avg_monthly_salary_usd: 10_000.0,
              duration_months: 12,
              overhead_pct: 0.3,
              projected_revenue_year1_usd: 200_000.0,
              projected_revenue_year2_usd: 500_000.0,
          };
          // monthly_burn = 4 * 10000 * 1.3 = 52000
          // total_cost   = 52000 * 12     = 624000
          // cumulative_revenue = 700000
          // net = 76000
          // roi = 76000 / 624000 * 100 ≈ 12.18%
          let result = tool.execute(args).await.unwrap();
          assert_eq!(result["monthly_burn_rate_usd"], 52_000.0);
          assert_eq!(result["total_development_cost_usd"], 624_000.0);
          assert_eq!(result["cumulative_revenue_24m_usd"], 700_000.0);
          assert_eq!(result["net_profit_loss_usd"], 76_000.0);
          // ROI = 12.18% — just check it's in the right ballpark
          let roi = result["roi_pct"].as_f64().unwrap();
          assert!((roi - 12.18).abs() < 0.02, "roi {roi}");
      }
  }
  ```

- [ ] **Step 2: Run test to verify it fails with `todo!()`**

  ```bash
  cargo test -p agentverse-demo-tools project_cost_estimator 2>&1 | tail -5
  ```

  Expected: `FAILED` with `not yet implemented`.

- [ ] **Step 3: Implement `execute` in `project_cost_estimator.rs`**

  Replace the `todo!()` body with:

  ```rust
  async fn execute(&self, args: ProjectCostArgs) -> ToolResult {
      let monthly_burn = args.team_size as f64
          * args.avg_monthly_salary_usd
          * (1.0 + args.overhead_pct);
      let total_cost = monthly_burn * args.duration_months as f64;
      let cumulative_revenue = args.projected_revenue_year1_usd
          + args.projected_revenue_year2_usd;
      let net = cumulative_revenue - total_cost;
      let roi_pct = if total_cost > 0.0 {
          (net / total_cost) * 100.0
      } else {
          0.0
      };
      Ok(json!({
          "total_development_cost_usd": round2(total_cost),
          "monthly_burn_rate_usd":      round2(monthly_burn),
          "cumulative_revenue_24m_usd": cumulative_revenue,
          "net_profit_loss_usd":        round2(net),
          "roi_pct":                    round2(roi_pct),
      }))
  }
  ```

  Add this helper at module level (above the struct):

  ```rust
  fn round2(v: f64) -> f64 {
      (v * 100.0).round() / 100.0
  }
  ```

- [ ] **Step 4: Run test to verify it passes**

  ```bash
  cargo test -p agentverse-demo-tools project_cost_estimator 2>&1 | tail -5
  ```

  Expected: `test tests::project_cost_estimator_computes_correctly ... ok`.

### NpvCalculator

- [ ] **Step 5: Replace `npv_calculator.rs` with the stub**

  ```rust
  use agentverse::{Tool, ToolResult};
  use schemars::JsonSchema;
  use serde::Deserialize;
  use serde_json::json;

  #[derive(Debug, Deserialize, JsonSchema)]
  pub struct NpvArgs {
      /// Initial investment in USD (positive number)
      pub initial_investment_usd: f64,
      /// Projected annual cash flows in USD, one per year
      pub annual_cash_flows_usd: Vec<f64>,
      /// Discount rate as a decimal (e.g. 0.1 for 10%)
      pub discount_rate_pct: f64,
  }

  pub struct NpvCalculator;

  fn round2(v: f64) -> f64 {
      (v * 100.0).round() / 100.0
  }

  fn npv_at_rate(flows: &[f64], rate: f64, investment: f64) -> f64 {
      let pv: f64 = flows
          .iter()
          .enumerate()
          .map(|(i, &cf)| cf / (1.0 + rate).powi(i as i32 + 1))
          .sum();
      pv - investment
  }

  #[async_trait::async_trait]
  impl Tool for NpvCalculator {
      type Args = NpvArgs;
      fn name(&self) -> &str { "npv_calculator" }
      fn description(&self) -> &str {
          "Calculate Net Present Value (NPV), approximate Internal Rate of Return (IRR), \
           and payback period for a project given initial investment, annual cash flows, \
           and a discount rate."
      }
      async fn execute(&self, _args: NpvArgs) -> ToolResult {
          todo!()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn npv_calculator_basic() {
          let tool = NpvCalculator;
          // $100k investment, $40k/year for 4 years, 10% discount rate
          // PV = 40000/1.1 + 40000/1.21 + 40000/1.331 + 40000/1.4641 ≈ 126794.62
          // NPV ≈ 26794.62
          // Payback: cumulative after yr3 = -100k + 40k*3 = 20k > 0 → payback = 3
          let args = NpvArgs {
              initial_investment_usd: 100_000.0,
              annual_cash_flows_usd: vec![40_000.0, 40_000.0, 40_000.0, 40_000.0],
              discount_rate_pct: 0.10,
          };
          let result = tool.execute(args).await.unwrap();
          let npv = result["npv_usd"].as_f64().unwrap();
          assert!((npv - 26_794.62).abs() < 1.0, "npv {npv}");
          assert_eq!(result["payback_period_years"], 3);
          // IRR should be ~21.86%
          let irr = result["irr_pct"].as_f64().unwrap();
          assert!((irr - 21.86).abs() < 0.5, "irr {irr}");
      }
  }
  ```

- [ ] **Step 6: Run test to verify it fails**

  ```bash
  cargo test -p agentverse-demo-tools npv_calculator 2>&1 | tail -5
  ```

  Expected: `FAILED` with `not yet implemented`.

- [ ] **Step 7: Implement `execute` in `npv_calculator.rs`**

  Replace `todo!()` with:

  ```rust
  async fn execute(&self, args: NpvArgs) -> ToolResult {
      let npv = npv_at_rate(
          &args.annual_cash_flows_usd,
          args.discount_rate_pct,
          args.initial_investment_usd,
      );

      // IRR via bisection: find rate where NPV = 0
      let irr = {
          let lo_val = npv_at_rate(&args.annual_cash_flows_usd, -0.99, args.initial_investment_usd);
          let hi_val = npv_at_rate(&args.annual_cash_flows_usd, 10.0, args.initial_investment_usd);
          if lo_val * hi_val < 0.0 {
              let mut lo = -0.99_f64;
              let mut hi = 10.0_f64;
              for _ in 0..50 {
                  let mid = (lo + hi) / 2.0;
                  if npv_at_rate(&args.annual_cash_flows_usd, mid, args.initial_investment_usd)
                      > 0.0
                  {
                      lo = mid;
                  } else {
                      hi = mid;
                  }
              }
              Some(round2((lo + hi) / 2.0 * 100.0))
          } else {
              None
          }
      };

      // Payback period: first year cumulative cash flow turns non-negative
      let mut cumulative = -args.initial_investment_usd;
      let mut payback_years: Option<usize> = None;
      let mut cash_flow_table = Vec::new();
      for (i, &cf) in args.annual_cash_flows_usd.iter().enumerate() {
          cumulative += cf;
          if payback_years.is_none() && cumulative >= 0.0 {
              payback_years = Some(i + 1);
          }
          cash_flow_table.push(json!({
              "year": i + 1,
              "cash_flow_usd": cf,
              "cumulative_usd": round2(cumulative),
          }));
      }

      Ok(json!({
          "npv_usd":              round2(npv),
          "irr_pct":              irr,
          "payback_period_years": payback_years,
          "cash_flows":           cash_flow_table,
      }))
  }
  ```

- [ ] **Step 8: Run tests to verify both tools pass**

  ```bash
  cargo test -p agentverse-demo-tools -- project_cost npv 2>&1 | tail -10
  ```

  Expected: 2 tests pass.

- [ ] **Step 9: Commit**

  ```bash
  git add examples/demo-tools/src/project_cost_estimator.rs \
          examples/demo-tools/src/npv_calculator.rs
  git commit -m "feat(demo-tools): add ProjectCostEstimator and NpvCalculator"
  ```

---

## Task 3: Schedule tools — `MilestoneScheduler` + `RiskAdjustedSchedule`

**Files:**
- Modify: `examples/demo-tools/src/milestone_scheduler.rs`
- Modify: `examples/demo-tools/src/risk_adjusted_schedule.rs`

### MilestoneScheduler

Inputs: `start_date` (YYYY-MM-DD), `phases[]` (name, duration_weeks, depends_on[]).
Outputs: per-phase start/end dates, project end date, total duration in weeks and months.

Each phase's start = `max(end_date of dependency phases)`, defaulting to `start_date` if no deps. Uses `chrono::NaiveDate` + `chrono::Duration::weeks()`.

### RiskAdjustedSchedule

Inputs: `phases[]` (name, optimistic_weeks, likely_weeks, pessimistic_weeks).
Outputs: per-phase expected duration + std dev; project total expected, total std dev, p80, p95.

Formulas (PERT):
- `E = (O + 4M + P) / 6`
- `σ = (P − O) / 6`
- `σ_total = √(Σ σ_i²)`
- `p80 = E_total + 0.84 × σ_total`
- `p95 = E_total + 1.65 × σ_total`

---

- [ ] **Step 1: Replace `milestone_scheduler.rs` with the stub**

  ```rust
  use agentverse::{Tool, ToolError, ToolResult};
  use chrono::{Duration, NaiveDate};
  use schemars::JsonSchema;
  use serde::Deserialize;
  use serde_json::json;
  use std::collections::HashMap;

  #[derive(Debug, Deserialize, JsonSchema)]
  pub struct Phase {
      /// Phase name (unique within this project)
      pub name: String,
      /// Phase duration in weeks
      pub duration_weeks: u32,
      /// Names of phases this phase depends on (must finish before this one starts)
      #[serde(default)]
      pub depends_on: Vec<String>,
  }

  #[derive(Debug, Deserialize, JsonSchema)]
  pub struct MilestoneArgs {
      /// Project start date in YYYY-MM-DD format
      pub start_date: String,
      pub phases: Vec<Phase>,
  }

  pub struct MilestoneScheduler;

  #[async_trait::async_trait]
  impl Tool for MilestoneScheduler {
      type Args = MilestoneArgs;
      fn name(&self) -> &str { "milestone_scheduler" }
      fn description(&self) -> &str {
          "Schedule project phases from a start date, respecting phase dependencies. \
           Returns per-phase start/end dates, total project duration in weeks and months."
      }
      async fn execute(&self, _args: MilestoneArgs) -> ToolResult {
          todo!()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn milestone_scheduler_sequential_phases() {
          let tool = MilestoneScheduler;
          let args = MilestoneArgs {
              start_date: "2026-01-01".into(),
              phases: vec![
                  Phase { name: "Discovery".into(), duration_weeks: 4, depends_on: vec![] },
                  Phase {
                      name: "MVP".into(),
                      duration_weeks: 8,
                      depends_on: vec!["Discovery".into()],
                  },
              ],
          };
          let result = tool.execute(args).await.unwrap();
          let phases = result["phases"].as_array().unwrap();
          assert_eq!(phases[0]["start"], "2026-01-01");
          assert_eq!(phases[0]["end"], "2026-01-29");   // +28 days
          assert_eq!(phases[1]["start"], "2026-01-29");
          assert_eq!(phases[1]["end"], "2026-03-26");   // +56 days
          assert_eq!(result["total_duration_weeks"], 12);
      }

      #[tokio::test]
      async fn milestone_scheduler_rejects_invalid_date() {
          let tool = MilestoneScheduler;
          let args = MilestoneArgs {
              start_date: "not-a-date".into(),
              phases: vec![],
          };
          assert!(tool.execute(args).await.is_err());
      }
  }
  ```

- [ ] **Step 2: Run tests to verify they fail**

  ```bash
  cargo test -p agentverse-demo-tools milestone 2>&1 | tail -5
  ```

  Expected: `FAILED`.

- [ ] **Step 3: Implement `execute` in `milestone_scheduler.rs`**

  ```rust
  async fn execute(&self, args: MilestoneArgs) -> ToolResult {
      let start = NaiveDate::parse_from_str(&args.start_date, "%Y-%m-%d").map_err(|_| {
          ToolError::Execution(format!(
              "Invalid date '{}': expected YYYY-MM-DD",
              args.start_date
          ))
      })?;

      let mut end_by_name: HashMap<String, NaiveDate> = HashMap::new();
      let mut schedule = Vec::new();

      for phase in &args.phases {
          let phase_start = phase
              .depends_on
              .iter()
              .filter_map(|dep| end_by_name.get(dep))
              .max()
              .copied()
              .unwrap_or(start);

          let phase_end = phase_start + Duration::weeks(phase.duration_weeks as i64);
          end_by_name.insert(phase.name.clone(), phase_end);

          schedule.push(json!({
              "name":           phase.name,
              "start":          phase_start.format("%Y-%m-%d").to_string(),
              "end":            phase_end.format("%Y-%m-%d").to_string(),
              "duration_weeks": phase.duration_weeks,
          }));
      }

      let project_end = end_by_name.values().max().copied().unwrap_or(start);
      let total_weeks = (project_end - start).num_weeks();

      Ok(json!({
          "phases":                    schedule,
          "project_end_date":          project_end.format("%Y-%m-%d").to_string(),
          "total_duration_weeks":      total_weeks,
          "total_duration_months":     ((total_weeks as f64 / 4.33).round() as i64),
      }))
  }
  ```

- [ ] **Step 4: Run tests to verify they pass**

  ```bash
  cargo test -p agentverse-demo-tools milestone 2>&1 | tail -5
  ```

  Expected: 2 tests pass.

### RiskAdjustedSchedule

- [ ] **Step 5: Replace `risk_adjusted_schedule.rs` with the stub**

  ```rust
  use agentverse::{Tool, ToolResult};
  use schemars::JsonSchema;
  use serde::Deserialize;
  use serde_json::json;

  #[derive(Debug, Deserialize, JsonSchema)]
  pub struct PertPhase {
      pub name: String,
      /// Best-case duration in weeks
      pub optimistic_weeks: f64,
      /// Most likely duration in weeks
      pub likely_weeks: f64,
      /// Worst-case duration in weeks
      pub pessimistic_weeks: f64,
  }

  #[derive(Debug, Deserialize, JsonSchema)]
  pub struct RiskScheduleArgs {
      pub phases: Vec<PertPhase>,
  }

  pub struct RiskAdjustedSchedule;

  fn round2(v: f64) -> f64 {
      (v * 100.0).round() / 100.0
  }

  #[async_trait::async_trait]
  impl Tool for RiskAdjustedSchedule {
      type Args = RiskScheduleArgs;
      fn name(&self) -> &str { "risk_adjusted_schedule" }
      fn description(&self) -> &str {
          "Apply PERT analysis to quantify schedule risk. Returns expected duration, \
           standard deviation, and p80/p95 confidence intervals for the total project."
      }
      async fn execute(&self, _args: RiskScheduleArgs) -> ToolResult {
          todo!()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn pert_single_phase() {
          // O=4, M=6, P=14
          // E = (4 + 24 + 14) / 6 = 7.0
          // σ = (14 - 4) / 6 ≈ 1.67
          // p80 = 7.0 + 0.84*1.67 ≈ 8.40
          // p95 = 7.0 + 1.65*1.67 ≈ 9.76
          let tool = RiskAdjustedSchedule;
          let args = RiskScheduleArgs {
              phases: vec![PertPhase {
                  name: "Build".into(),
                  optimistic_weeks: 4.0,
                  likely_weeks: 6.0,
                  pessimistic_weeks: 14.0,
              }],
          };
          let result = tool.execute(args).await.unwrap();
          assert_eq!(result["total_expected_weeks"], 7.0);
          let p80 = result["p80_weeks"].as_f64().unwrap();
          let p95 = result["p95_weeks"].as_f64().unwrap();
          assert!((p80 - 8.40).abs() < 0.02, "p80 {p80}");
          assert!((p95 - 9.76).abs() < 0.02, "p95 {p95}");
      }
  }
  ```

- [ ] **Step 6: Run test to verify it fails**

  ```bash
  cargo test -p agentverse-demo-tools risk_adjusted 2>&1 | tail -5
  ```

  Expected: `FAILED`.

- [ ] **Step 7: Implement `execute` in `risk_adjusted_schedule.rs`**

  ```rust
  async fn execute(&self, args: RiskScheduleArgs) -> ToolResult {
      let mut total_mean = 0.0_f64;
      let mut total_variance = 0.0_f64;

      let phase_results: Vec<_> = args
          .phases
          .iter()
          .map(|p| {
              let mean = (p.optimistic_weeks + 4.0 * p.likely_weeks + p.pessimistic_weeks) / 6.0;
              let sd = (p.pessimistic_weeks - p.optimistic_weeks) / 6.0;
              total_mean += mean;
              total_variance += sd * sd;
              json!({
                  "name":           p.name,
                  "expected_weeks": round2(mean),
                  "std_dev_weeks":  round2(sd),
              })
          })
          .collect();

      let total_sd = total_variance.sqrt();

      Ok(json!({
          "phases":               phase_results,
          "total_expected_weeks": round2(total_mean),
          "total_std_dev_weeks":  round2(total_sd),
          "p80_weeks":            round2(total_mean + 0.84 * total_sd),
          "p95_weeks":            round2(total_mean + 1.65 * total_sd),
      }))
  }
  ```

- [ ] **Step 8: Run all schedule tool tests**

  ```bash
  cargo test -p agentverse-demo-tools -- milestone risk_adjusted 2>&1 | tail -10
  ```

  Expected: 3 tests pass.

- [ ] **Step 9: Commit**

  ```bash
  git add examples/demo-tools/src/milestone_scheduler.rs \
          examples/demo-tools/src/risk_adjusted_schedule.rs
  git commit -m "feat(demo-tools): add MilestoneScheduler and RiskAdjustedSchedule"
  ```

---

## Task 4: Market tools — `MarketSizingCalculator` + `RunwayProjector`

**Files:**
- Modify: `examples/demo-tools/src/market_sizing_calculator.rs`
- Modify: `examples/demo-tools/src/runway_projector.rs`

### MarketSizingCalculator

Inputs: TAM, segment fraction, capture fraction, years to SOM.
Outputs: TAM, SAM (TAM × segment), SOM (SAM × capture), annual/monthly revenue at full capture.

### RunwayProjector

Inputs: initial funding, monthly burn, current monthly revenue, MoM revenue growth rate.
Outputs: runway months, break-even month, cash snapshots at months 12/18/24, series-A readiness signal.

Series-A ready: `runway_months > 12 AND breakeven_month <= 18`.

---

- [ ] **Step 1: Replace `market_sizing_calculator.rs` with the stub**

  ```rust
  use agentverse::{Tool, ToolResult};
  use schemars::JsonSchema;
  use serde::Deserialize;
  use serde_json::json;

  #[derive(Debug, Deserialize, JsonSchema)]
  pub struct MarketSizingArgs {
      /// Total Addressable Market in USD
      pub total_addressable_market_usd: f64,
      /// Fraction of TAM reachable by this product (e.g. 0.1 for 10%)
      pub target_segment_pct: f64,
      /// Fraction of SAM to capture (e.g. 0.05 for 5%)
      pub capture_rate_pct: f64,
      /// Years expected to reach full SOM
      pub years_to_som: u32,
  }

  pub struct MarketSizingCalculator;

  fn round2(v: f64) -> f64 {
      (v * 100.0).round() / 100.0
  }

  #[async_trait::async_trait]
  impl Tool for MarketSizingCalculator {
      type Args = MarketSizingArgs;
      fn name(&self) -> &str { "market_sizing_calculator" }
      fn description(&self) -> &str {
          "Calculate TAM, SAM, and SOM from market size and capture assumptions. \
           Returns implied annual and monthly revenue targets at full market capture."
      }
      async fn execute(&self, _args: MarketSizingArgs) -> ToolResult {
          todo!()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn market_sizing_tam_sam_som() {
          let tool = MarketSizingCalculator;
          let args = MarketSizingArgs {
              total_addressable_market_usd: 1_000_000_000.0,
              target_segment_pct: 0.1,
              capture_rate_pct: 0.05,
              years_to_som: 3,
          };
          // SAM = 1B * 0.1 = 100M
          // SOM = 100M * 0.05 = 5M
          let result = tool.execute(args).await.unwrap();
          assert_eq!(result["tam_usd"], 1_000_000_000.0);
          assert_eq!(result["sam_usd"], 100_000_000.0);
          assert_eq!(result["som_usd"], 5_000_000.0);
          assert_eq!(result["monthly_revenue_target_usd"], 5_000_000.0 / 12.0);
      }
  }
  ```

- [ ] **Step 2: Run test to verify it fails**

  ```bash
  cargo test -p agentverse-demo-tools market_sizing 2>&1 | tail -5
  ```

  Expected: `FAILED`.

- [ ] **Step 3: Implement `execute` in `market_sizing_calculator.rs`**

  ```rust
  async fn execute(&self, args: MarketSizingArgs) -> ToolResult {
      let tam = args.total_addressable_market_usd;
      let sam = tam * args.target_segment_pct;
      let som = sam * args.capture_rate_pct;
      Ok(json!({
          "tam_usd":                           tam,
          "sam_usd":                           round2(sam),
          "som_usd":                           round2(som),
          "annual_revenue_at_som_usd":         round2(som),
          "monthly_revenue_target_usd":        round2(som / 12.0),
          "years_to_som":                      args.years_to_som,
      }))
  }
  ```

### RunwayProjector

- [ ] **Step 4: Replace `runway_projector.rs` with the stub**

  ```rust
  use agentverse::{Tool, ToolResult};
  use schemars::JsonSchema;
  use serde::Deserialize;
  use serde_json::json;

  #[derive(Debug, Deserialize, JsonSchema)]
  pub struct RunwayArgs {
      /// Starting cash balance in USD
      pub initial_funding_usd: f64,
      /// Fixed monthly expenses in USD
      pub monthly_burn_usd: f64,
      /// Current monthly revenue in USD
      pub monthly_revenue_usd: f64,
      /// Month-over-month revenue growth as a decimal (e.g. 0.1 for 10%)
      pub monthly_revenue_growth_pct: f64,
  }

  pub struct RunwayProjector;

  fn round2(v: f64) -> f64 {
      (v * 100.0).round() / 100.0
  }

  #[async_trait::async_trait]
  impl Tool for RunwayProjector {
      type Args = RunwayArgs;
      fn name(&self) -> &str { "runway_projector" }
      fn description(&self) -> &str {
          "Project cash runway, break-even month, and cash position over time given \
           initial funding, monthly burn, current revenue, and MoM revenue growth rate."
      }
      async fn execute(&self, _args: RunwayArgs) -> ToolResult {
          todo!()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn runway_projector_flat_burn() {
          // $200k funding, $10k burn, $0 revenue, 0% growth
          // Cash exhausted after month 20 (200k / 10k = 20 months)
          let tool = RunwayProjector;
          let args = RunwayArgs {
              initial_funding_usd: 200_000.0,
              monthly_burn_usd: 10_000.0,
              monthly_revenue_usd: 0.0,
              monthly_revenue_growth_pct: 0.0,
          };
          let result = tool.execute(args).await.unwrap();
          assert_eq!(result["runway_months"], 20);
          assert!(result["breakeven_month"].is_null());
          assert_eq!(result["series_a_ready"], false);
      }
  }
  ```

- [ ] **Step 5: Run test to verify it fails**

  ```bash
  cargo test -p agentverse-demo-tools runway 2>&1 | tail -5
  ```

  Expected: `FAILED`.

- [ ] **Step 6: Implement `execute` in `runway_projector.rs`**

  ```rust
  async fn execute(&self, args: RunwayArgs) -> ToolResult {
      const MAX_MONTHS: usize = 48;
      let mut cash = args.initial_funding_usd;
      let mut revenue = args.monthly_revenue_usd;
      let mut runway_months: Option<usize> = None;
      let mut breakeven_month: Option<usize> = None;
      let mut snapshots = Vec::new();

      for month in 1..=MAX_MONTHS {
          revenue *= 1.0 + args.monthly_revenue_growth_pct;
          let net = revenue - args.monthly_burn_usd;
          cash += net;

          if breakeven_month.is_none() && revenue >= args.monthly_burn_usd {
              breakeven_month = Some(month);
          }
          if runway_months.is_none() && cash <= 0.0 {
              runway_months = Some(month);
          }

          if [12, 18, 24].contains(&month) {
              snapshots.push(json!({
                  "month": month,
                  "cash_usd": round2(cash),
                  "monthly_revenue_usd": round2(revenue),
              }));
          }
      }

      let runway = runway_months.unwrap_or(MAX_MONTHS);
      let series_a_ready = runway > 12
          && breakeven_month.map_or(false, |b| b <= 18);

      Ok(json!({
          "runway_months":    runway,
          "breakeven_month":  breakeven_month,
          "cash_snapshots":   snapshots,
          "series_a_ready":   series_a_ready,
      }))
  }
  ```

- [ ] **Step 7: Run all tests in the crate**

  ```bash
  cargo test -p agentverse-demo-tools 2>&1 | tail -15
  ```

  Expected: 6 tests pass (one per tool).

- [ ] **Step 8: Commit**

  ```bash
  git add examples/demo-tools/src/market_sizing_calculator.rs \
          examples/demo-tools/src/runway_projector.rs \
          examples/demo-tools/src/lib.rs
  git commit -m "feat(demo-tools): add MarketSizingCalculator and RunwayProjector"
  ```

---

## Task 5: `examples/project-feasibility`

**Files:**
- Create: `examples/project-feasibility/Cargo.toml`
- Create: `examples/project-feasibility/prompts/react.j2`
- Create: `examples/project-feasibility/src/main.rs`

**What it does:** Parses a project description from the CLI, starts an MCP server with 4 of the 6 tools, connects a client to discover them, runs 3 analyst subagents in parallel via `run_many`, collects results as `ResourceContent`, then runs 1 synthesis subagent to produce a structured feasibility report (PROCEED / HOLD / REJECT verdict).

---

- [ ] **Step 1: Create `examples/project-feasibility/Cargo.toml`**

  ```toml
  [package]
  name = "example-project-feasibility"
  version.workspace = true
  edition.workspace = true
  rust-version.workspace = true

  [dependencies]
  agentverse              = { path = "../../avs-core" }
  agentverse-demo-tools   = { path = "../demo-tools" }
  agentverse-logging      = { path = "../../avs-logging" }
  agentverse-mcp          = { path = "../../avs-mcp" }
  agentverse-subagent     = { path = "../../avs-subagent" }
  agentverse-tools        = { path = "../../avs-tools" }
  tokio   = { workspace = true, features = ["time"] }
  tracing = { workspace = true }
  ```

- [ ] **Step 2: Create `examples/project-feasibility/prompts/react.j2`**

  ```jinja2
  Available tools:
  {{ tools }}

  Respond using this format:

      Thought: <reasoning>
      Action: <tool_name>
      Action Input: <valid JSON matching the tool's input schema>

  When you have a final answer:

      Thought: <summary of findings>
      Answer: <your complete answer>
  ```

- [ ] **Step 3: Create `examples/project-feasibility/src/main.rs`**

  ```rust
  use agentverse::{ConnectionManager, PromptConfig, PromptRegistry};
  use agentverse_demo_tools::{
      MilestoneScheduler, NpvCalculator, ProjectCostEstimator, RiskAdjustedSchedule,
  };
  use agentverse_logging as avs_logging;
  use agentverse_mcp::{McpCatalogSource, McpClient, McpServer, McpTransport};
  use agentverse_subagent::{Budget, ResourceContent, SubAgentContext, SubAgentExecutor, SubAgentSpec};
  use agentverse_tools::ToolRegistry;
  use std::sync::Arc;
  use std::time::Duration;

  #[tokio::main]
  async fn main() {
      avs_logging::init();

      let args: Vec<String> = std::env::args().collect();
      if args.len() < 2 {
          eprintln!("Usage: {} \"<project description>\"", args[0]);
          std::process::exit(1);
      }
      let project = args[1..].join(" ");

      let base_url = std::env::var("MODEL_BASE_URL")
          .unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
      let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
      let model_name = std::env::var("MODEL_NAME")
          .unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

      // ── 1. MCP server ──────────────────────────────────────────────────────
      let server_registry = ToolRegistry::new();
      server_registry.register(ProjectCostEstimator);
      server_registry.register(NpvCalculator);
      server_registry.register(MilestoneScheduler);
      server_registry.register(RiskAdjustedSchedule);

      let mut server = McpServer::new(Arc::clone(&server_registry));
      let port = server.bind_random_port().await.expect("bind MCP server");
      tokio::spawn(async move { server.run().await });
      tokio::time::sleep(Duration::from_millis(50)).await;

      // ── 2. MCP client → discover tools ────────────────────────────────────
      let transport = McpTransport::StreamableHttp {
          endpoint: format!("http://127.0.0.1:{port}/mcp")
              .parse()
              .expect("endpoint"),
          headers: Default::default(),
      };
      let client = McpClient::connect(transport).await.expect("connect MCP");
      let mcp_tools = ToolRegistry::new();
      let discovered = McpCatalogSource::populate(&mcp_tools, &client)
          .await
          .expect("populate MCP tools");
      tracing::info!(discovered, "MCP tools discovered");

      // ── 3. Executor ────────────────────────────────────────────────────────
      let cm = Arc::new(ConnectionManager::openai(&base_url, &model_name, &api_key));
      let prompts = Arc::new(
          PromptRegistry::from_config(&PromptConfig {
              prompts_dir: Some(
                  concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string(),
              ),
              ..Default::default()
          })
          .expect("prompts"),
      );
      let executor = SubAgentExecutor::new(cm, mcp_tools, prompts);

      // ── 4. Stage 1: three analysts in parallel ─────────────────────────────
      let base_ctx = SubAgentContext { resources: vec![], depth: 0 };

      let tasks = vec![
          (
              SubAgentSpec {
                  name: "financial-analyst".into(),
                  system_prompt: Some(
                      "You are a financial analyst. Use project_cost_estimator to estimate \
                       total development cost and npv_calculator to evaluate long-term return. \
                       Be specific with numbers."
                          .into(),
                  ),
                  objective: format!(
                      "Estimate total development cost, NPV, and 3-year ROI for: {}. \
                       Assume 12% discount rate and 18-month development duration.",
                      project
                  ),
                  model: None,
                  allowed_tools: vec![
                      "project_cost_estimator".into(),
                      "npv_calculator".into(),
                  ],
                  budget: Budget {
                      max_steps: 8,
                      max_tokens: 4000,
                      timeout: Duration::from_secs(90),
                  },
              },
              base_ctx.clone(),
          ),
          (
              SubAgentSpec {
                  name: "timeline-analyst".into(),
                  system_prompt: Some(
                      "You are a project timeline analyst. Use milestone_scheduler to \
                       project delivery phases from today's date."
                          .into(),
                  ),
                  objective: format!(
                      "Project a realistic delivery timeline for: {}. \
                       Start from today. Include phases: Discovery (4w), MVP (12w), \
                       Beta (8w), GA (4w). Each phase depends on the previous.",
                      project
                  ),
                  model: None,
                  allowed_tools: vec!["milestone_scheduler".into()],
                  budget: Budget {
                      max_steps: 5,
                      max_tokens: 3000,
                      timeout: Duration::from_secs(60),
                  },
              },
              base_ctx.clone(),
          ),
          (
              SubAgentSpec {
                  name: "risk-analyst".into(),
                  system_prompt: Some(
                      "You are a risk analyst. Use risk_adjusted_schedule to quantify \
                       schedule uncertainty. Identify the top 5 technical and business risks."
                          .into(),
                  ),
                  objective: format!(
                      "Identify top 5 risks and quantify schedule risk using PERT \
                       estimates for: {}",
                      project
                  ),
                  model: None,
                  allowed_tools: vec!["risk_adjusted_schedule".into()],
                  budget: Budget {
                      max_steps: 6,
                      max_tokens: 3000,
                      timeout: Duration::from_secs(60),
                  },
              },
              base_ctx.clone(),
          ),
      ];

      println!("\nRunning 3 analyst subagents in parallel...\n");
      let results = executor.run_many(tasks).await;

      // ── 5. Collect results as ResourceContent ──────────────────────────────
      let labels = ["Financial Analysis", "Timeline Analysis", "Risk Analysis"];
      let mut resources = Vec::new();
      for (label, result) in labels.iter().zip(results.iter()) {
          match result {
              Ok(r) => {
                  println!("  [ok] {} ({} steps)", label, r.steps);
                  resources.push(ResourceContent {
                      label: label.to_string(),
                      content: r.answer.clone(),
                  });
              }
              Err(e) => {
                  println!("  [err] {}: {}", label, e);
                  resources.push(ResourceContent {
                      label: label.to_string(),
                      content: "[analysis unavailable]".into(),
                  });
              }
          }
      }

      // ── 6. Stage 2: synthesis ──────────────────────────────────────────────
      println!("\nSynthesizing feasibility report...\n");
      let synthesis_spec = SubAgentSpec {
          name: "synthesis".into(),
          system_prompt: Some(
              "You are a senior consultant. Synthesize multi-domain analyses into a \
               clear executive report with an actionable recommendation."
                  .into(),
          ),
          objective: "Based on the financial, timeline, and risk analyses in the context, \
                      write a structured Project Feasibility Report with sections: \
                      Executive Summary, Financial Outlook, Delivery Timeline, Risk Profile, \
                      and a final verdict — PROCEED / HOLD / REJECT — with justification."
              .into(),
          model: None,
          allowed_tools: vec![],
          budget: Budget {
              max_steps: 5,
              max_tokens: 6000,
              timeout: Duration::from_secs(90),
          },
      };
      let synthesis_ctx = SubAgentContext { resources, depth: 0 };

      match executor.run(&synthesis_spec, synthesis_ctx).await {
          Ok(r) => println!("{}", r.answer),
          Err(e) => eprintln!("Synthesis failed: {}", e),
      }
  }
  ```

- [ ] **Step 4: Verify the binary compiles**

  ```bash
  cargo build -p example-project-feasibility 2>&1 | tail -10
  ```

  Expected: `Finished` with no errors.

- [ ] **Step 5: Commit**

  ```bash
  git add examples/project-feasibility/
  git commit -m "feat(project-feasibility): programmatic run_many subagent pipeline demo"
  ```

---

## Task 6: `examples/business-report`

**Files:**
- Create: `examples/business-report/Cargo.toml`
- Create: `examples/business-report/prompts/react.j2`
- Create: `examples/business-report/prompts/system.j2`
- Create: `examples/business-report/skills/system/business-report/SKILL.md`
- Create: `examples/business-report/src/main.rs`

**What it does:** Builds a `SkillConfig` from the `skills/` directory, registers `SubAgentTool` into the Agent's tool registry, and starts a conversational session. When the user's input matches the `business-report` skill, the LLM is instructed to spawn three analyst subagents via `spawn_subagent`, wait for their answers, and write a synthesis.

---

- [ ] **Step 1: Create `examples/business-report/Cargo.toml`**

  ```toml
  [package]
  name = "example-business-report"
  version.workspace = true
  edition.workspace = true
  rust-version.workspace = true

  [dependencies]
  agentverse              = { path = "../../avs-core" }
  agentverse-agent        = { path = "../../avs-agent" }
  agentverse-demo-tools   = { path = "../demo-tools" }
  agentverse-logging      = { path = "../../avs-logging" }
  agentverse-mcp          = { path = "../../avs-mcp" }
  agentverse-session      = { path = "../../avs-session" }
  agentverse-strategy     = { path = "../../avs-strategy" }
  agentverse-subagent     = { path = "../../avs-subagent" }
  agentverse-tools        = { path = "../../avs-tools" }
  tokio   = { workspace = true, features = ["time"] }
  tracing = { workspace = true }
  ```

- [ ] **Step 2: Create `examples/business-report/prompts/react.j2`**

  Same template as project-feasibility — format only, no persona:

  ```jinja2
  Available tools:
  {{ tools }}

  Respond using this format:

      Thought: <reasoning>
      Action: <tool_name>
      Action Input: <valid JSON matching the tool's input schema>

  When you have a final answer:

      Thought: <summary of findings>
      Answer: <your complete answer>
  ```

- [ ] **Step 3: Create `examples/business-report/prompts/system.j2`**

  ```jinja2
  You are a business intelligence orchestrator. You coordinate specialist
  subagents to produce comprehensive, multi-domain business analyses.
  ```

- [ ] **Step 4: Create `examples/business-report/skills/system/business-report/SKILL.md`**

  The `skills/system/` path matches the skill loader's expected structure. The `agentverse:` frontmatter block lists `spawn_subagent` as a required tool so the router activates this skill only when the tool is registered.

  ```markdown
  ---
  name: business-report
  description: >
    Produces a multi-domain business report by spawning specialist analyst subagents.
    Use when the user asks for a business report, company analysis, or product assessment.
  version: 1.0.0
  agentverse:
    tools:
      - spawn_subagent
  ---

  # Business Report

  You produce business reports by orchestrating three specialist subagents, then
  synthesizing their findings into a structured report.

  ## Step 1 — spawn three analysts

  Call `spawn_subagent` three times (sequentially or in one turn if your LLM
  supports parallel tool calls), substituting the user's subject for `{subject}`:

  **market-analyst**
  ```json
  {
    "name": "market-analyst",
    "objective": "Assess the market opportunity for {subject}. Use market_sizing_calculator to size TAM/SAM/SOM. Estimate realistic capture rates and time to reach SOM.",
    "system_prompt": "You are a market research analyst. Use market_sizing_calculator to quantify the market opportunity with specific numbers.",
    "allowed_tools": ["market_sizing_calculator"],
    "max_steps": 6,
    "max_tokens": 3000,
    "timeout_secs": 60
  }
  ```

  **financial-analyst**
  ```json
  {
    "name": "financial-analyst",
    "objective": "Project the financial trajectory for {subject}. Use runway_projector to model cash runway and break-even given realistic funding, burn, and revenue growth assumptions.",
    "system_prompt": "You are a financial analyst. Use runway_projector to model cash runway and break-even timing with specific numbers.",
    "allowed_tools": ["runway_projector"],
    "max_steps": 6,
    "max_tokens": 3000,
    "timeout_secs": 60
  }
  ```

  **operations-analyst**
  ```json
  {
    "name": "operations-analyst",
    "objective": "Map the operational build-out plan for {subject}. Use milestone_scheduler to project phases from today and risk_adjusted_schedule to quantify delivery uncertainty.",
    "system_prompt": "You are an operations analyst. Use milestone_scheduler and risk_adjusted_schedule to plan and de-risk the build-out.",
    "allowed_tools": ["milestone_scheduler", "risk_adjusted_schedule"],
    "max_steps": 8,
    "max_tokens": 4000,
    "timeout_secs": 90
  }
  ```

  ## Step 2 — synthesize

  After all three analysts have responded, write a **Business Report** with:

  1. **Executive Summary** — 3-sentence overview
  2. **Market Opportunity** — from market-analyst findings
  3. **Financial Outlook** — from financial-analyst findings
  4. **Operations Plan** — from operations-analyst findings
  5. **Recommendation** — INVEST / MONITOR / PASS with one-paragraph justification
  ```

  > **Note:** The triple-backtick blocks inside this Markdown file are valid — Markdown allows nested fenced code blocks if the outer fence uses a different number of backticks, or the inner blocks are inside a list item at an indented level. When creating this file, copy the raw content exactly.

- [ ] **Step 5: Create `examples/business-report/src/main.rs`**

  ```rust
  use agentverse::{Config, ConnectionManager, LlmRunner, PromptConfig, PromptRegistry, ProviderConfig};
  use agentverse_agent::{Agent, SkillConfig, SkillMode};
  use agentverse_demo_tools::{
      MarketSizingCalculator, MilestoneScheduler, RiskAdjustedSchedule, RunwayProjector,
  };
  use agentverse_logging as avs_logging;
  use agentverse_mcp::{McpCatalogSource, McpClient, McpServer, McpTransport};
  use agentverse_session::SqliteSessionMemory;
  use agentverse_strategy::{build, StrategyKind};
  use agentverse_subagent::SubAgentExecutor;
  use agentverse_tools::ToolRegistry;
  use std::sync::Arc;
  use std::time::Duration;

  #[tokio::main]
  async fn main() {
      avs_logging::init();

      let args: Vec<String> = std::env::args().collect();
      if args.len() < 2 {
          eprintln!("Usage: {} \"<company or product description>\"", args[0]);
          std::process::exit(1);
      }
      let subject = args[1..].join(" ");

      let base_url = std::env::var("MODEL_BASE_URL")
          .unwrap_or_else(|_| "http://localhost:9090/v1".to_string());
      let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
      let model_name = std::env::var("MODEL_NAME")
          .unwrap_or_else(|_| "Qwen3.6-35B-A3B-GGUF".to_string());

      // ── 1. MCP server ──────────────────────────────────────────────────────
      let server_registry = ToolRegistry::new();
      server_registry.register(MarketSizingCalculator);
      server_registry.register(RunwayProjector);
      server_registry.register(MilestoneScheduler);
      server_registry.register(RiskAdjustedSchedule);

      let mut server = McpServer::new(Arc::clone(&server_registry));
      let port = server.bind_random_port().await.expect("bind MCP server");
      tokio::spawn(async move { server.run().await });
      tokio::time::sleep(Duration::from_millis(50)).await;

      // ── 2. MCP client → discover tools ────────────────────────────────────
      let transport = McpTransport::StreamableHttp {
          endpoint: format!("http://127.0.0.1:{port}/mcp")
              .parse()
              .expect("endpoint"),
          headers: Default::default(),
      };
      let client = McpClient::connect(transport).await.expect("connect MCP");
      let mcp_tools = ToolRegistry::new();
      McpCatalogSource::populate(&mcp_tools, &client)
          .await
          .expect("populate MCP tools");

      // ── 3. SubAgentExecutor with MCP tools ─────────────────────────────────
      // Subagents access the domain tools; the main agent gets only spawn_subagent.
      let cm = Arc::new(ConnectionManager::openai(&base_url, &model_name, &api_key));
      let prompts = Arc::new(
          PromptRegistry::from_config(&PromptConfig {
              prompts_dir: Some(
                  concat!(env!("CARGO_MANIFEST_DIR"), "/prompts").to_string(),
              ),
              ..Default::default()
          })
          .expect("prompts"),
      );
      let executor = Arc::new(SubAgentExecutor::new(
          cm,
          mcp_tools,
          Arc::clone(&prompts),
      ));

      // ── 4. Agent tool registry: spawn_subagent only ────────────────────────
      let agent_tools = ToolRegistry::new();
      SubAgentExecutor::register_tool(&executor, &agent_tools);

      // ── 5. LlmRunner for the main agent ───────────────────────────────────
      let runner = Arc::new(
          LlmRunner::from_config(Config {
              provider: ProviderConfig::OpenAI {
                  model_name: model_name.clone(),
                  api_key: api_key.clone(),
                  base_url: Some(base_url.clone()),
              },
              max_messages: 100,
              tools: vec![],
              prompts_dir: None,
              system_prompt: None,
          })
          .expect("runner"),
      );

      // ── 6. Skill ───────────────────────────────────────────────────────────
      let skills_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/skills");
      let skills = SkillConfig::load(skills_dir, SkillMode::Constrained(vec![
          "business-report".to_string(),
      ]))
      .expect("load skills");

      // ── 7. Agent ───────────────────────────────────────────────────────────
      let strategy = build(
          StrategyKind::React,
          Arc::clone(&runner),
          Arc::clone(&prompts),
          Arc::clone(&agent_tools),
          15,
      );
      let session_memory = Arc::new(
          SqliteSessionMemory::new("sqlite::memory:")
              .await
              .expect("session memory"),
      );
      let agent = Agent::new(
          runner,
          agent_tools,
          prompts,
          session_memory,
          strategy,
          false,
          None,
          Some(skills),
      );

      // ── 8. Invoke ──────────────────────────────────────────────────────────
      let question = format!("Generate a business report for: {}", subject);
      println!("> {}\n", question);

      let session_id = agent
          .create_session("user")
          .await
          .expect("create session");

      match agent.invoke("user", session_id, &question).await {
          Ok(answer) => println!("{}", answer),
          Err(e) => eprintln!("Error: {}", e),
      }
  }
  ```

- [ ] **Step 6: Verify the binary compiles**

  ```bash
  cargo build -p example-business-report 2>&1 | tail -10
  ```

  Expected: `Finished` with no errors.

  `SkillMode` is re-exported by `agentverse_agent` — the import `use agentverse_agent::{Agent, SkillConfig, SkillMode}` above is correct and no `agentverse-skill` dependency is required.

- [ ] **Step 7: Run `cargo test` across all new crates to confirm nothing regressed**

  ```bash
  cargo test -p agentverse-demo-tools -p example-project-feasibility -p example-business-report 2>&1 | tail -15
  ```

  Expected: 6 unit tests in `agentverse-demo-tools` pass; the two example crates have no tests (they're integration-only), so `0 tests` is correct for them.

- [ ] **Step 8: Run `cargo clippy` and `cargo fmt --check`**

  ```bash
  cargo clippy -p agentverse-demo-tools -p example-project-feasibility -p example-business-report -- -D warnings 2>&1 | tail -20
  cargo fmt --all --check 2>&1 | tail -5
  ```

  Fix any warnings before committing.

- [ ] **Step 9: Commit**

  ```bash
  git add examples/business-report/
  git commit -m "feat(business-report): LLM-driven skill + spawn_subagent orchestration demo"
  ```

---

## Run instructions

### `project-feasibility`

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
cargo run -p example-project-feasibility -- \
  "A real-time collaborative code editor with AI suggestions"
```

### `business-report`

```bash
MODEL_BASE_URL=http://localhost:9090/v1 \
MODEL_NAME=Qwen3.6-35B-A3B-GGUF \
cargo run -p example-business-report -- \
  "A SaaS platform for restaurant inventory management"
```

Both default to an empty `MODEL_API_KEY`, which is correct for a local LLM that doesn't require authentication.
