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

/// Parse the LLM response to extract thought, action(s), or answer.
///
/// Scans line-by-line (trimming each line) so indented output from models
/// that follow an indented format example is handled correctly.
/// ToolCall(s) take priority over Answer to prevent returning hallucinated answers.
/// Multiple Action/Action Input pairs produce a ToolCalls batch for parallel dispatch.
pub fn parse_response(response: &str) -> CycleAction {
    let lines: Vec<&str> = response.lines().collect();
    let mut answer_idx: Option<usize> = None;
    let mut tool_calls: Vec<agentverse::ToolCall> = Vec::new();
    let mut pending_action: Option<String> = None;

    for (i, &line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if lower.starts_with("answer:") && answer_idx.is_none() {
            answer_idx = Some(i);
        } else if lower.starts_with("action input:") {
            let raw = trimmed["Action Input:".len()..].trim().to_string();
            let args = serde_json::from_str(&raw).unwrap_or(Value::Null);
            if let Some(name) = pending_action.take() {
                tool_calls.push(agentverse::ToolCall { name, args });
            }
        } else if lower.starts_with("action:") {
            // Flush unpaired action (no Action Input line following)
            if let Some(name) = pending_action.take() {
                tool_calls.push(agentverse::ToolCall {
                    name,
                    args: Value::Null,
                });
            }
            let val = trimmed["Action:".len()..].trim().to_string();
            if !val.is_empty() {
                pending_action = Some(val);
            }
        }
    }
    // Flush trailing unpaired action
    if let Some(name) = pending_action.take() {
        tool_calls.push(agentverse::ToolCall {
            name,
            args: Value::Null,
        });
    }

    // ToolCall(s) take priority over Answer: if the model hallucinated a full
    // Thought→Action→Observation→Answer trace in one shot, we still dispatch
    // the real tool rather than returning the hallucinated answer.
    if !tool_calls.is_empty() {
        return if tool_calls.len() == 1 {
            let c = tool_calls.remove(0);
            CycleAction::ToolCall {
                tool_name: c.name,
                args: c.args,
            }
        } else {
            CycleAction::ToolCalls { calls: tool_calls }
        };
    }

    // Answer captures everything from the Answer: label to end-of-response so
    // multi-line answers (bullet lists, etc.) are not truncated.
    if let Some(idx) = answer_idx {
        let first = lines[idx].trim()["answer:".len()..].trim();
        let rest = lines[idx + 1..].join("\n");
        let answer = if rest.trim().is_empty() {
            first.to_string()
        } else if first.is_empty() {
            rest.trim().to_string()
        } else {
            format!("{}\n{}", first, rest).trim().to_string()
        };
        if !answer.is_empty() {
            return CycleAction::Done { answer };
        }
    }

    // Strip a leading "Thought: " prefix so the caller doesn't double-wrap it.
    let raw = response.trim();
    let thought = if raw.to_lowercase().starts_with("thought:") {
        raw["thought:".len()..].trim().to_string()
    } else {
        raw.to_string()
    };
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
    fn test_parse_answer_only() {
        let result = parse_response("Thought: done.\nAnswer: The answer is 42");
        match result {
            CycleAction::Done { answer } => assert_eq!(answer, "The answer is 42"),
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tool_call_overrides_hallucinated_answer() {
        // Model generates a full Thought→Action→Observation→Answer trace in one
        // shot, hallucinating the Observation.  ToolCall must win so the real
        // tool is dispatched instead of returning the hallucinated answer.
        let result = parse_response(
            "Thought: I need the current time.\nAction: datetime\nAction Input: {}\nObservation: 2024-01-01 00:00:00\nAnswer: It is 2024-01-01.",
        );
        match result {
            CycleAction::ToolCall { tool_name, args } => {
                assert_eq!(tool_name, "datetime");
                assert_eq!(args, Value::Object(serde_json::Map::new()));
            }
            other => panic!("Expected ToolCall, got {:?}", other),
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

    #[test]
    fn test_parse_multiple_tool_calls() {
        let response = "Thought: I need two things.\n\
                        Action: calculator\n\
                        Action Input: {\"operation\": \"add\", \"a\": 1, \"b\": 2}\n\
                        Action: datetime\n\
                        Action Input: {}";
        let action = parse_response(response);
        match action {
            CycleAction::ToolCalls { calls } => {
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].name, "calculator");
                assert_eq!(calls[1].name, "datetime");
            }
            other => panic!("Expected ToolCalls, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_single_tool_still_works() {
        let response =
            "Action: calculator\nAction Input: {\"operation\": \"add\", \"a\": 1, \"b\": 2}";
        let action = parse_response(response);
        match action {
            CycleAction::ToolCall { tool_name, .. } => assert_eq!(tool_name, "calculator"),
            CycleAction::ToolCalls { calls } => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "calculator");
            }
            other => panic!("Expected tool call, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_multiline_answer() {
        let result = parse_response(
            "Answer: Here is what I can do:\n- Calculator: math\n- DateTime: current time",
        );
        match result {
            CycleAction::Done { answer } => {
                assert!(answer.contains("Here is what I can do:"));
                assert!(answer.contains("- Calculator: math"));
                assert!(answer.contains("- DateTime: current time"));
            }
            other => panic!("Expected Done, got {:?}", other),
        }
    }
}
