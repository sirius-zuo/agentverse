//! Run with: cargo test -p agentverse-memory-pgvector -- --ignored
use agentverse::memory::{Message, MessageRole};
use agentverse_memory_pgvector::PostgresSessionMemory;
use agentverse_session::store::SessionMemory;

const TEST_DB: &str = "postgresql://localhost/agentverse_test";

#[tokio::test]
#[ignore = "requires live PostgreSQL at localhost/agentverse_test"]
async fn postgres_create_and_get_session() {
    let store = PostgresSessionMemory::new(TEST_DB).await.unwrap();
    let session = store.create("pg-user-1").await.unwrap();
    let fetched = store.get(session.id).await.unwrap().unwrap();
    assert_eq!(fetched.user_id, "pg-user-1");
}

#[tokio::test]
#[ignore = "requires live PostgreSQL at localhost/agentverse_test"]
async fn postgres_append_and_load_messages() {
    let store = PostgresSessionMemory::new(TEST_DB).await.unwrap();
    let session = store.create("pg-user-2").await.unwrap();
    store
        .append_message(
            session.id,
            Message::text(MessageRole::User, "postgres test"),
        )
        .await
        .unwrap();
    let messages = store.load_messages(session.id).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].as_text(), "postgres test");
}

#[tokio::test]
#[ignore = "requires live PostgreSQL at localhost/agentverse_test"]
async fn postgres_append_turn_persists_both_in_order() {
    let store = PostgresSessionMemory::new(TEST_DB).await.unwrap();
    let session = store.create("pg-user-3").await.unwrap();
    store
        .append_turn(
            session.id,
            Message::text(MessageRole::User, "hello postgres"),
            Message::text(MessageRole::Assistant, "hi postgres"),
        )
        .await
        .unwrap();
    let messages = store.load_messages(session.id).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].as_text(), "hello postgres");
    assert_eq!(messages[1].as_text(), "hi postgres");
}
