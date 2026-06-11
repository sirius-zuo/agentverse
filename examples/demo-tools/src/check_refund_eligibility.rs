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
    fn name(&self) -> &str {
        "check_refund_eligibility"
    }
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
        let result = tool
            .execute(CheckRefundEligibilityArgs {
                invoice_id: "1042".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(result["eligible"], true);
        assert!(result["refund_amount_usd"].as_f64().unwrap() > 0.0);
    }
}
