use agentverse::{Tool, ToolResult};
use crate::round2;
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
