use agentverse::Message;

/// Summarize a list of messages into a shorter form.
/// This is a local function — the actual summary generation
/// should be done by the LLM. This provides a fallback truncation.
pub fn truncate_messages(messages: &[Message], max_tokens: usize) -> String {
    let mut result = String::new();
    let mut token_count = 0;

    // Keep most recent messages
    for msg in messages.iter().rev() {
        let msg_str = format!("{:?}: {}\n", msg.role, msg.content);
        let chars = msg_str.chars().count();
        if token_count + chars > max_tokens {
            break;
        }
        result.insert_str(0, &msg_str);
        token_count += chars;
    }

    if token_count > 0 {
        result.insert_str(0, "[Summary of older messages]\n");
    }

    result
}

/// Detect if auto-summary should be triggered.
/// Default threshold: 50 messages
pub fn should_trigger_summary(message_count: usize, threshold: usize) -> bool {
    message_count >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_messages_empty() {
        let messages: Vec<Message> = vec![];
        let result = truncate_messages(&messages, 100);
        assert_eq!(result, "");
    }

    #[test]
    fn test_truncate_messages_within_limit() {
        let messages = vec![
            Message {
                role: agentverse::MessageRole::User,
                content: "Hello".to_string(),
            },
            Message {
                role: agentverse::MessageRole::Assistant,
                content: "Hi there!".to_string(),
            },
        ];
        let result = truncate_messages(&messages, 1000);
        assert!(result.contains("[Summary of older messages]"));
        assert!(result.contains("User: Hello"));
        assert!(result.contains("Assistant: Hi there!"));
    }

    #[test]
    fn test_truncate_messages_exceed_limit() {
        let messages = vec![
            Message {
                role: agentverse::MessageRole::User,
                content: "Hello".to_string(),
            },
            Message {
                role: agentverse::MessageRole::Assistant,
                content: "Hi there!".to_string(),
            },
        ];
        let result = truncate_messages(&messages, 5);
        // With a very tight limit, no messages fit, so no summary header
        assert_eq!(result, "");
    }

    #[test]
    fn test_should_trigger_summary_below_threshold() {
        assert!(!should_trigger_summary(40, 50));
    }

    #[test]
    fn test_should_trigger_summary_at_threshold() {
        assert!(should_trigger_summary(50, 50));
    }

    #[test]
    fn test_should_trigger_summary_above_threshold() {
        assert!(should_trigger_summary(100, 50));
    }
}
