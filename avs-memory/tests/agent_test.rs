use agentverse::memory::{Memory, Message, MessageRole};
use agentverse_memory::{AgentMemory, NoopBackend, NoopSummarizer};

fn user_msg(content: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: content.to_string(),
    }
}

fn make_agent_memory(max: usize, threshold: usize) -> AgentMemory<NoopSummarizer, NoopBackend> {
    AgentMemory::new(max, threshold, NoopSummarizer, NoopBackend)
}

#[tokio::test]
async fn test_agent_memory_basic_append_and_last_n() {
    let mut m = make_agent_memory(10, 100);
    m.append(user_msg("a"));
    m.append(user_msg("b"));
    let result = m.last_n(5).await.unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].content, "a");
    assert_eq!(result[1].content, "b");
}

#[tokio::test]
async fn test_agent_memory_pin_survives_eviction() {
    let mut m = make_agent_memory(2, 100);
    m.pin(vec![user_msg("preamble")]);
    for i in 0..5 {
        m.append(user_msg(&format!("msg{}", i)));
    }
    let result = m.last_n(10).await.unwrap();
    assert_eq!(result[0].content, "preamble");
    assert_eq!(result.len(), 3); // 1 pinned + 2 window
}

#[tokio::test]
async fn test_agent_memory_summarization_triggered() {
    // threshold=3: after 3 appends, needs_summarization=true
    let mut m = make_agent_memory(20, 3);
    m.append(user_msg("a"));
    m.append(user_msg("b"));
    m.append(user_msg("c")); // triggers flag
                             // last_n should summarize oldest half and replace with "[summary]"
    let result = m.last_n(10).await.unwrap();
    // After summarization: oldest half replaced by 1 summary message
    // window had [a,b,c], oldest half = [a] (len/2=1), so window becomes [[summary],b,c]
    assert!(result.iter().any(|m| m.content == "[summary]"));
}

#[tokio::test]
async fn test_agent_memory_summarization_failure_degrades_gracefully() {
    use agentverse::memory::{MemoryError, Message};
    use agentverse_memory::traits::Summarizer;
    use async_trait::async_trait;

    struct FailingSummarizer;
    #[async_trait]
    impl Summarizer for FailingSummarizer {
        async fn summarize(&self, _msgs: &[Message]) -> Result<Message, MemoryError> {
            Err(MemoryError::Summarization(
                "intentional failure".to_string(),
            ))
        }
    }

    let mut m = AgentMemory::new(20, 3, FailingSummarizer, NoopBackend);
    m.append(user_msg("a"));
    m.append(user_msg("b"));
    m.append(user_msg("c")); // triggers flag
                             // Even with failing summarizer, last_n must succeed
    let result = m.last_n(10).await.unwrap();
    // All original messages present (summarization was a no-op)
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].content, "a");
}

#[tokio::test]
async fn test_agent_memory_clear() {
    let mut m = make_agent_memory(10, 100);
    m.pin(vec![user_msg("pin")]);
    m.append(user_msg("msg"));
    m.clear();
    let result = m.last_n(10).await.unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_agent_memory_flush() {
    let mut m = make_agent_memory(10, 100);
    m.append(user_msg("msg"));
    m.flush().await.unwrap(); // NoopBackend — just verify no error
                              // Data still present after flush
    let result = m.last_n(10).await.unwrap();
    assert_eq!(result.len(), 1);
}
