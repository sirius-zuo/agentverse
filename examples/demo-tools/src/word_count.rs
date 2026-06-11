use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WordCountArgs {
    /// The text to count words in
    pub text: String,
}

pub struct WordCount;

#[async_trait::async_trait]
impl Tool for WordCount {
    type Args = WordCountArgs;
    fn name(&self) -> &str { "word_count" }
    fn description(&self) -> &str {
        "Count the number of whitespace-separated words in a text string. Returns an integer."
    }
    async fn execute(&self, args: WordCountArgs) -> ToolResult {
        let count = args.text.split_whitespace().count() as u64;
        Ok(json!(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn word_count_counts_tokens() {
        let tool = WordCount;
        let result = tool.execute(WordCountArgs {
            text: "one two three four five".to_string(),
        })
        .await
        .unwrap();
        assert_eq!(result, json!(5));
    }

    #[tokio::test]
    async fn word_count_empty_string_returns_zero() {
        let tool = WordCount;
        let result = tool.execute(WordCountArgs { text: String::new() })
            .await
            .unwrap();
        assert_eq!(result, json!(0));
    }
}
