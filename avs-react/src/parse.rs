//! LLM response parser for ReAct format.
//!
//! Parses the LLM output to extract thoughts, tool calls, or final answers.
//!
//! Expected formats:
//! - `Thought: xxx\nAction: tool_name\nAction Input: {...}`
//! - `Thought: xxx\nAnswer: final answer`
//! - Just `Thought: xxx`

use super::cycle::CycleAction;
use serde_json::Value;

/// Parse the LLM response to extract thought, action, or answer.
///
/// Checks for "Answer:" first (highest priority), then "Action:",
/// and defaults to treating the whole response as a thought.
pub fn parse_response(response: &str) -> CycleAction {
    let lower = response.to_lowercase();

    // Check for Answer: first (highest priority)
    if let Some(pos) = lower.find("\nanswer:") {
        let answer = response[pos + 8..].trim().to_string();
        if !answer.is_empty() {
            return CycleAction::Done { answer };
        }
    }

    // Also check for "Answer:" at the start of the response
    if let Some(stripped) = response.strip_prefix("Answer:") {
        let answer = stripped.trim().to_string();
        if !answer.is_empty() {
            return CycleAction::Done { answer };
        }
    }

    // Check for Action:
    if let Some(pos) = lower.find("\naction:") {
        let tool_name = extract_tool_name(response, pos);
        let args = extract_args(response);

        return CycleAction::ToolCall { tool_name, args };
    }

    // Also check for "Action:" at the start
    if let Some(stripped) = response.strip_prefix("Action:") {
        let tool_name = stripped.trim().to_string();
        if !tool_name.is_empty() {
            let args = extract_args(response);
            return CycleAction::ToolCall { tool_name, args };
        }
    }

    // Just a thought
    let thought = response.trim().to_string();
    if thought.is_empty() {
        CycleAction::Error {
            message: "Empty response from model".to_string(),
        }
    } else {
        CycleAction::Continue { thought }
    }
}

fn extract_tool_name(response: &str, action_pos: usize) -> String {
    let after_action = &response[action_pos..];
    for line in after_action.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("action:") {
            return trimmed["Action:".len()..].trim().to_string();
        }
    }
    "unknown".to_string()
}

fn extract_args(response: &str) -> Value {
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("action input:") {
            let json_str = trimmed["Action Input:".len()..].trim();
            if let Ok(value) = serde_json::from_str(json_str) {
                return value;
            }
        }
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_answer_with_newline() {
        let result = parse_response("Thought: I know the answer.\nAnswer: Hello world");
        match result {
            CycleAction::Done { answer } => assert_eq!(answer, "Hello world"),
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_answer_no_newline() {
        let result = parse_response("Answer: 42");
        match result {
            CycleAction::Done { answer } => assert_eq!(answer, "42"),
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tool_call() {
        let result = parse_response(
            "Thought: Let me search.\nAction: file_search\nAction Input: {\"path\": \".\"}",
        );
        match result {
            CycleAction::ToolCall { tool_name, args } => {
                assert_eq!(tool_name, "file_search");
                assert_eq!(args["path"], ".");
            }
            other => panic!("Expected ToolCall, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tool_call_no_args() {
        let result = parse_response("Thought: Let me list.\nAction: list_files");
        match result {
            CycleAction::ToolCall { tool_name, args } => {
                assert_eq!(tool_name, "list_files");
                assert_eq!(args, Value::Null);
            }
            other => panic!("Expected ToolCall, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_thought_only() {
        let result = parse_response("I should first check the directory structure");
        match result {
            CycleAction::Continue { thought } => {
                assert_eq!(
                    thought,
                    "I should first check the directory structure"
                );
            }
            other => panic!("Expected Continue, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_empty_response() {
        let result = parse_response("");
        match result {
            CycleAction::Error { message } => assert!(message.contains("Empty")),
            other => panic!("Expected Error for empty response, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_answer_overrides_action() {
        // Answer takes priority over Action
        let result = parse_response("Thought: done.\nAnswer: The answer is 42");
        match result {
            CycleAction::Done { answer } => assert_eq!(answer, "The answer is 42"),
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_thought_with_action_keyword_in_text() {
        // Action mentioned in thought text should not trigger tool call
        let result = parse_response(
            "I need to think about what the action should be.\nAnswer: it depends",
        );
        match result {
            CycleAction::Done { answer } => assert_eq!(answer, "it depends"),
            other => panic!("Expected Done, got {:?}", other),
        }
    }
}
