use crate::round2;
use agentverse::{Tool, ToolError, ToolResult};
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

#[async_trait::async_trait]
impl Tool for RiskAdjustedSchedule {
    type Args = RiskScheduleArgs;
    fn name(&self) -> &str {
        "risk_adjusted_schedule"
    }
    fn description(&self) -> &str {
        "Apply PERT analysis to quantify schedule risk. Returns expected duration, \
         standard deviation, and p80/p95 confidence intervals for the total project."
    }
    async fn execute(&self, args: RiskScheduleArgs) -> ToolResult {
        for p in &args.phases {
            if p.optimistic_weeks > p.pessimistic_weeks {
                return Err(ToolError::Execution(format!(
                    "Phase '{}': optimistic_weeks ({}) must be ≤ pessimistic_weeks ({})",
                    p.name, p.optimistic_weeks, p.pessimistic_weeks
                )));
            }
        }

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

    #[tokio::test]
    async fn pert_rejects_inverted_range() {
        // optimistic (10w) > pessimistic (4w) is an invalid PERT input
        let tool = RiskAdjustedSchedule;
        let args = RiskScheduleArgs {
            phases: vec![PertPhase {
                name: "Build".into(),
                optimistic_weeks: 10.0,
                likely_weeks: 7.0,
                pessimistic_weeks: 4.0,
            }],
        };
        assert!(tool.execute(args).await.is_err());
    }
}
