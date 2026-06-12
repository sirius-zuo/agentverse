use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LedgerPostArgs {
    /// Journal entry reference (e.g. "je-2026-06").
    pub entry_id: String,
    /// Human-readable description of the entry.
    pub description: String,
    /// Net amount in USD (negative = credit).
    pub amount_usd: f64,
}

pub struct LedgerPost;

#[async_trait::async_trait]
impl Tool for LedgerPost {
    type Args = LedgerPostArgs;
    fn name(&self) -> &str {
        "ledger_post"
    }
    fn description(&self) -> &str {
        "Post a journal entry to the accounting ledger. Returns the posted entry with a confirmation number."
    }
    async fn execute(&self, args: LedgerPostArgs) -> ToolResult {
        Ok(json!({
            "confirmation": format!("CONF-{}", args.entry_id.to_uppercase()),
            "entry_id":     args.entry_id,
            "description":  args.description,
            "amount_usd":   args.amount_usd,
            "status":       "posted",
            "posted_at":    "2026-06-12T00:00:00Z"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ledger_post_returns_posted_status() {
        let tool = LedgerPost;
        let result = tool
            .execute(LedgerPostArgs {
                entry_id:    "je-2026-06".to_string(),
                description: "June payroll".to_string(),
                amount_usd:  -15000.00,
            })
            .await
            .unwrap();
        assert_eq!(result["status"], "posted");
        assert_eq!(result["entry_id"], "je-2026-06");
        assert_eq!(result["confirmation"], "CONF-JE-2026-06");
        assert_eq!(result["amount_usd"], -15000.00);
    }
}
