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
    Text {
        /// The text payload.
        text: String,
    },
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
            ContentBlock::Text { text } => text.clone(),
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// Flatten this message's content to plain text. Escape hatch for
    /// consumers that only need text (embeddings, logging, guardrails).
    pub fn as_text(&self) -> String {
        content_as_text(&self.content)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        assert_eq!(
            msg.content,
            vec![ContentBlock::Text {
                text: "hello".to_string()
            }]
        );
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
                ContentBlock::Text {
                    text: "checking the schedule".to_string(),
                },
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

    #[test]
    fn serde_roundtrip_text_block() {
        let text_block = ContentBlock::Text {
            text: "hello world".to_string(),
        };

        // Serialize to JSON
        let json =
            serde_json::to_string(&text_block).expect("Text block should serialize successfully");

        // Verify the shape: should be {"type":"text","text":"hello world"}
        assert_eq!(json, r#"{"type":"text","text":"hello world"}"#);

        // Deserialize back
        let deserialized: ContentBlock =
            serde_json::from_str(&json).expect("Should deserialize back to Text block");

        // Verify round-trip
        assert_eq!(deserialized, text_block);
    }

    #[test]
    fn serde_roundtrip_tool_use_block() {
        let tool_use = ContentBlock::ToolUse {
            id: "call_xyz".to_string(),
            name: "fetch_data".to_string(),
            input: serde_json::json!({"url": "https://example.com"}),
        };

        let json =
            serde_json::to_string(&tool_use).expect("ToolUse block should serialize successfully");

        let deserialized: ContentBlock =
            serde_json::from_str(&json).expect("Should deserialize back to ToolUse block");

        assert_eq!(deserialized, tool_use);
    }

    #[test]
    fn serde_roundtrip_tool_result_block() {
        let tool_result = ContentBlock::ToolResult {
            tool_use_id: "call_xyz".to_string(),
            content: r#"{"status": "success", "data": [1, 2, 3]}"#.to_string(),
            is_error: false,
        };

        let json = serde_json::to_string(&tool_result)
            .expect("ToolResult block should serialize successfully");

        let deserialized: ContentBlock =
            serde_json::from_str(&json).expect("Should deserialize back to ToolResult block");

        assert_eq!(deserialized, tool_result);
    }

    #[test]
    fn serde_roundtrip_message_with_mixed_content() {
        let msg = Message {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Let me check that for you.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "search".to_string(),
                    input: serde_json::json!({"query": "example"}),
                },
            ],
        };

        let json = serde_json::to_string(&msg)
            .expect("Message with mixed content should serialize successfully");

        let deserialized: Message =
            serde_json::from_str(&json).expect("Should deserialize back to Message");

        assert_eq!(deserialized, msg);
    }
}
