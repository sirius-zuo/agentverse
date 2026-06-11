use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CountMentionsArgs {
    /// The term to count (case-insensitive substring match per whitespace token)
    pub term: String,
    /// The text to search
    pub text: String,
}

pub struct CountMentions;

#[async_trait::async_trait]
impl Tool for CountMentions {
    type Args = CountMentionsArgs;
    fn name(&self) -> &str { "count_mentions" }
    fn description(&self) -> &str {
        "Count how many whitespace-separated tokens in text contain the term \
         (case-insensitive). Returns an integer."
    }
    async fn execute(&self, args: CountMentionsArgs) -> ToolResult {
        let needle = args.term.to_lowercase();
        let count = args
            .text
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.contains(needle.as_str()))
            .count() as u64;
        Ok(json!(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn count_mentions_is_case_insensitive() {
        let tool = CountMentions;
        let result = tool.execute(CountMentionsArgs {
            term: "rust".to_string(),
            text: "Rust is great. I love rust. RUST is fast.".to_string(),
        })
        .await
        .unwrap();
        assert_eq!(result, json!(3));
    }

    #[tokio::test]
    async fn count_mentions_returns_zero_for_no_match() {
        let tool = CountMentions;
        let result = tool.execute(CountMentionsArgs {
            term: "python".to_string(),
            text: "Rust is great. I love rust.".to_string(),
        })
        .await
        .unwrap();
        assert_eq!(result, json!(0));
    }
}
