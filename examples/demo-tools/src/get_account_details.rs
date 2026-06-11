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
        })
        .await
        .unwrap();
        assert_eq!(result["plan"], "Pro");
        assert_eq!(result["status"], "active");
        assert_eq!(result["seats"], 5);
    }
}
