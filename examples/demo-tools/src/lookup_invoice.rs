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
    fn name(&self) -> &str {
        "lookup_invoice"
    }
    fn description(&self) -> &str {
        "Look up an invoice by ID. Returns invoice amount, date, status, and plan."
    }
    async fn execute(&self, args: LookupInvoiceArgs) -> ToolResult {
        Ok(json!({
            "invoice_id":    args.invoice_id,
            "amount_usd":    99.00,
            "date":          "2026-06-01",
            "status":        "paid",
            "plan":          "Pro",
            "description":   "Monthly subscription — Pro plan"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_invoice_returns_paid_invoice() {
        let tool = LookupInvoice;
        let result = tool
            .execute(LookupInvoiceArgs {
                invoice_id: "1042".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(result["status"], "paid");
        assert_eq!(result["amount_usd"], 99.00);
        assert_eq!(result["invoice_id"], "1042");
    }
}
