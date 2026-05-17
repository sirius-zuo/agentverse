use agentverse::memory::{Message, MessageRole};
use agentverse_memory::{NoopBackend, NoopSummarizer, Summarizer, LongTermBackend};

#[tokio::test]
async fn test_noop_summarizer_returns_summary_message() {
    let s = NoopSummarizer;
    let msgs = vec![Message { role: MessageRole::User, content: "hi".to_string() }];
    let result = s.summarize(&msgs).await.unwrap();
    assert_eq!(result.content, "[summary]");
}

#[tokio::test]
async fn test_noop_backend_store_and_search() {
    let b = NoopBackend;
    let msg = Message { role: MessageRole::User, content: "hi".to_string() };
    b.store(msg, vec![]).await.unwrap();
    let results = b.search(vec![], 5).await.unwrap();
    assert!(results.is_empty());
}
