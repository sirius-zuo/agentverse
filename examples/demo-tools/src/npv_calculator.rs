use agentverse::{Tool, ToolResult};
use crate::round2;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

fn npv_at_rate(flows: &[f64], rate: f64, investment: f64) -> f64 {
    let pv: f64 = flows
        .iter()
        .enumerate()
        .map(|(i, &cf)| cf / (1.0 + rate).powi(i as i32 + 1))
        .sum();
    pv - investment
}

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

#[async_trait::async_trait]
impl Tool for NpvCalculator {
    type Args = NpvArgs;
    fn name(&self) -> &str { "npv_calculator" }
    fn description(&self) -> &str {
        "Calculate Net Present Value (NPV), approximate Internal Rate of Return (IRR), \
         and payback period for a project given initial investment, annual cash flows, \
         and a discount rate."
    }
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
