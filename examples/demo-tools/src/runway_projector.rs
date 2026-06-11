use agentverse::{Tool, ToolError, ToolResult};
use crate::round2;
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

#[async_trait::async_trait]
impl Tool for RunwayProjector {
    type Args = RunwayArgs;
    fn name(&self) -> &str { "runway_projector" }
    fn description(&self) -> &str {
        "Project cash runway, break-even month, and cash position over time given \
         initial funding, monthly burn, current revenue, and MoM revenue growth rate."
    }
    async fn execute(&self, args: RunwayArgs) -> ToolResult {
        if args.monthly_revenue_usd == 0.0 && args.monthly_revenue_growth_pct > 0.0 {
            return Err(ToolError::Execution(
                "monthly_revenue_usd must be > 0 when monthly_revenue_growth_pct > 0; \
                 multiplicative growth cannot generate revenue from a zero baseline"
                    .into(),
            ));
        }

        const MAX_MONTHS: usize = 48;
        let mut cash = args.initial_funding_usd;
        let mut revenue = args.monthly_revenue_usd;
        let mut runway_months: Option<usize> = None;
        let mut breakeven_month: Option<usize> = None;
        let mut snapshots = Vec::new();

        for month in 1..=MAX_MONTHS {
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

            revenue *= 1.0 + args.monthly_revenue_growth_pct;
        }

        let beyond_horizon = runway_months.is_none();
        let runway = runway_months.unwrap_or(MAX_MONTHS);
        let series_a_ready = (beyond_horizon || runway > 12)
            && breakeven_month.map_or(false, |b| b <= 18);

        Ok(json!({
            "runway_months":    runway,
            "beyond_horizon":   beyond_horizon,
            "breakeven_month":  breakeven_month,
            "cash_snapshots":   snapshots,
            "series_a_ready":   series_a_ready,
        }))
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

    #[tokio::test]
    async fn runway_projector_rejects_zero_revenue_with_positive_growth() {
        // 0 * (1 + 0.1) = 0 forever — tool must reject rather than silently return wrong output
        let tool = RunwayProjector;
        let args = RunwayArgs {
            initial_funding_usd: 500_000.0,
            monthly_burn_usd: 10_000.0,
            monthly_revenue_usd: 0.0,
            monthly_revenue_growth_pct: 0.1,
        };
        assert!(tool.execute(args).await.is_err());
    }

    #[tokio::test]
    async fn runway_projector_growth_starts_month_two() {
        // Revenue=$4800, burn=$5000, growth=5%/month
        // Fixed:  month 1 uses $4800 (< break-even); month 2 uses $5040 (> burn) → breakeven=2
        // Buggy:  month 1 grows first → uses $5040 → breakeven=1
        let tool = RunwayProjector;
        let args = RunwayArgs {
            initial_funding_usd: 1_000_000.0,
            monthly_burn_usd: 5_000.0,
            monthly_revenue_usd: 4_800.0,
            monthly_revenue_growth_pct: 0.05,
        };
        let result = tool.execute(args).await.unwrap();
        assert_eq!(result["breakeven_month"], 2);
    }

    #[tokio::test]
    async fn runway_projector_beyond_horizon_flag() {
        // $500k funding, $10k burn, $20k revenue, 0% growth → cash grows forever
        // beyond_horizon must be true; runway_months=48 is a sentinel, not definite runway
        let tool = RunwayProjector;
        let args = RunwayArgs {
            initial_funding_usd: 500_000.0,
            monthly_burn_usd: 10_000.0,
            monthly_revenue_usd: 20_000.0,
            monthly_revenue_growth_pct: 0.0,
        };
        let result = tool.execute(args).await.unwrap();
        assert_eq!(result["beyond_horizon"], true);
        assert_eq!(result["runway_months"], 48);
    }
}
