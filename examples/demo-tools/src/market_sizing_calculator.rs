use agentverse::{Tool, ToolResult};
use crate::round2;
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

#[async_trait::async_trait]
impl Tool for MarketSizingCalculator {
    type Args = MarketSizingArgs;
    fn name(&self) -> &str { "market_sizing_calculator" }
    fn description(&self) -> &str {
        "Calculate TAM, SAM, and SOM from market size and capture assumptions. \
         Returns implied annual and monthly revenue targets at full market capture."
    }
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
        let monthly = result["monthly_revenue_target_usd"].as_f64().unwrap();
        assert!((monthly - 5_000_000.0 / 12.0).abs() < 0.01, "monthly {monthly}");
    }
}
