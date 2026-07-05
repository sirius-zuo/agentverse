use agentverse::memory::{Message, MessageRole};
use agentverse_memory::{LongTermBackend, NoopBackend};

#[tokio::test]
async fn test_noop_backend_store_and_search() {
    let b = NoopBackend;
    let msg = Message {
        role: MessageRole::User,
        content: "hi".to_string(),
    };
    b.store(msg, vec![]).await.unwrap();
    let results = b.search(vec![], 5).await.unwrap();
    assert!(results.is_empty());
}
