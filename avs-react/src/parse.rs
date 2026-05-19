//! LLM response parser for ReAct format.
//!
//! Parses the LLM output to extract thoughts, tool calls, or final answers.
//!
//! Expected formats:
//! - `Thought: xxx\nAction: tool_name\nAction Input: {...}`
//! - `Thought: xxx\nAnswer: final answer`
//! - Just `Thought: xxx`
//!
//! Leading whitespace is stripped from each line before matching, so templates
//! that show indented examples (e.g. `    Action: ...`) work correctly.

use super::cycle::CycleAction;
use serde_json::Value;

/// Parse the LLM response to extract thought, action, or answer.
///
/// Scans line-by-line (trimming each line) so indented output from models
/// that follow an indented format example is handled correctly.
/// Answer takes priority over Action when both appear.
pub fn parse_response(response: &str) -> CycleAction {
    let mut found_answer: Option<String> = None;
    let mut found_action: Option<String> = None;
    let mut found_action_input: Option<String> = None;

    for line in response.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if lower.starts_with("answer:") && found_answer.is_none() {
            let val = trimmed["Answer:".len()..].trim().to_string();
            if !val.is_empty() {
                found_answer = Some(val);
            }
        } else if lower.starts_with("action input:") && found_action_input.is_none() {
            found_action_input = Some(trimmed["Action Input:".len()..].trim().to_string());
        } else if lower.starts_with("action:") && found_action.is_none() {
            let val = trimmed["Action:".len()..].trim().to_string();
            if !val.is_empty() {
                found_action = Some(val);
            }
        }
    }

    if let Some(answer) = found_answer {
        return CycleAction::Done { answer };
    }

    if let Some(tool_name) = found_action {
        let args = found_action_input
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        return CycleAction::ToolCall { tool_name, args };
    }

    let thought = response.trim().to_string();
    if thought.is_empty() {
        CycleAction::Error {
            message: "Empty response from model".to_string(),
        }
    } else {
        CycleAction::Continue { thought }
    }
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
                assert_eq!(thought, "I should first check the directory structure");
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
        let result = parse_response("Thought: done.\nAnswer: The answer is 42");
        match result {
            CycleAction::Done { answer } => assert_eq!(answer, "The answer is 42"),
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_thought_with_action_keyword_in_text() {
        let result =
            parse_response("I need to think about what the action should be.\nAnswer: it depends");
        match result {
            CycleAction::Done { answer } => assert_eq!(answer, "it depends"),
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_indented_action() {
        // Model follows indented template format — parser must still detect Action:
        let result = parse_response(
            "Thought: I need to multiply.\n    Action: calculator\n    Action Input: {\"operation\": \"multiply\", \"a\": 42, \"b\": 37}",
        );
        match result {
            CycleAction::ToolCall { tool_name, args } => {
                assert_eq!(tool_name, "calculator");
                assert_eq!(args["operation"], "multiply");
                assert_eq!(args["a"], 42);
            }
            other => panic!("Expected ToolCall, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_indented_answer() {
        let result = parse_response("Thought: Done.\n    Answer: 1569");
        match result {
            CycleAction::Done { answer } => assert_eq!(answer, "1569"),
            other => panic!("Expected Done, got {:?}", other),
        }
    }
}
