use agentverse::{Tool, ToolResult};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindDatesArgs {
    /// The text to search for date patterns
    pub text: String,
}

pub struct FindDates;

#[async_trait::async_trait]
impl Tool for FindDates {
    type Args = FindDatesArgs;
    fn name(&self) -> &str { "find_dates" }
    fn description(&self) -> &str {
        "Find date patterns (YYYY-MM-DD and M/D/YYYY) in a text string. \
         Returns a JSON array of matched date strings."
    }
    async fn execute(&self, args: FindDatesArgs) -> ToolResult {
        let re = Regex::new(r"\b(\d{4}-\d{2}-\d{2}|\d{1,2}/\d{1,2}/\d{4})\b").unwrap();
        let dates: Vec<&str> = re.find_iter(&args.text).map(|m| m.as_str()).collect();
        Ok(json!(dates))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn find_dates_returns_iso_and_us_formats() {
        let tool = FindDates;
        let result = tool.execute(FindDatesArgs {
            text: "Meeting on 2024-03-15 rescheduled to 4/10/2024.".to_string(),
        })
        .await
        .unwrap();
        let dates: Vec<serde_json::Value> = result.as_array().unwrap().to_vec();
        assert_eq!(dates.len(), 2);
        assert!(dates.iter().any(|d| d == "2024-03-15"));
        assert!(dates.iter().any(|d| d == "4/10/2024"));
    }

    #[tokio::test]
    async fn find_dates_returns_empty_when_no_dates() {
        let tool = FindDates;
        let result = tool.execute(FindDatesArgs {
            text: "No dates in this text at all.".to_string(),
        })
        .await
        .unwrap();
        assert!(result.as_array().unwrap().is_empty());
    }
}
