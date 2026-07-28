use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A single piece of message content: text, a tool call, or a tool result.
///
/// This is the atomic unit of content exchanged with language models. `Message`
/// contains a vec of these; model responses produce them as part of the output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text(String),
    /// A tool call issued by the model.
    ///
    /// The `id` field is issued by the provider (the model's response) and must
    /// be echoed back verbatim as the `tool_use_id` in the matching `ToolResult`
    /// in the next turn — this is how the provider correlates the result to the call.
    ToolUse {
        /// Unique identifier for this tool call, issued by the provider.
        id: String,
        /// Name of the tool to invoke.
        name: String,
        /// Tool input as JSON.
        input: serde_json::Value,
    },
    /// Result of executing a tool call.
    ToolResult {
        /// The `id` from the `ToolUse` call this result answers.
        tool_use_id: String,
        /// Pre-serialized tool output as a string. This is the shape both
        /// Anthropic and OpenAI wire protocols expect, not a structured `Value`.
        content: String,
        /// Whether this result represents an error.
        is_error: bool,
    },
}

/// Flattens content blocks into plain text: `Text` blocks verbatim, `ToolUse`/
/// `ToolResult` blocks as a short human-readable summary. Shared by
/// `Message::as_text` and `GenerateResponse::as_text`.
pub fn content_as_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .map(|block| match block {
            ContentBlock::Text(text) => text.clone(),
            ContentBlock::ToolUse { name, .. } => format!("[tool_use: {name}]"),
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => format!("[tool_result {tool_use_id}: {content}]"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Construct a plain-text message — the common case for user input, system
    /// prompts, and final text answers.
    pub fn text(role: MessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text(text.into())],
        }
    }

    /// Flatten this message's content to plain text. Escape hatch for
    /// consumers that only need text (embeddings, logging, guardrails).
    pub fn as_text(&self) -> String {
        content_as_text(&self.content)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("Summarization failed: {0}")]
    Summarization(String),
    #[error("Storage failed: {0}")]
    Storage(String),
    #[error("Retrieval failed: {0}")]
    Retrieval(String),
    #[error("Embedding failed: {0}")]
    Embedding(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_constructor_wraps_a_single_text_block() {
        let msg = Message::text(MessageRole::User, "hello");
        assert_eq!(msg.content, vec![ContentBlock::Text("hello".to_string())]);
    }

    #[test]
    fn as_text_flattens_a_single_text_block() {
        let msg = Message::text(MessageRole::Assistant, "hi there");
        assert_eq!(msg.as_text(), "hi there");
    }

    #[test]
    fn as_text_summarizes_tool_use_and_tool_result_blocks() {
        let assistant_turn = Message {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text("checking the schedule".to_string()),
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "milestone_scheduler".to_string(),
                    input: serde_json::json!({"start_date": "2026-01-01"}),
                },
            ],
        };
        assert_eq!(
            assistant_turn.as_text(),
            "checking the schedule\n[tool_use: milestone_scheduler]"
        );

        let tool_turn = Message {
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: "{\"total_weeks\": 12}".to_string(),
                is_error: false,
            }],
        };
        assert_eq!(
            tool_turn.as_text(),
            "[tool_result call_1: {\"total_weeks\": 12}]"
        );
    }

    #[test]
    fn content_as_text_handles_empty_slice() {
        assert_eq!(content_as_text(&[]), "");
    }
}
