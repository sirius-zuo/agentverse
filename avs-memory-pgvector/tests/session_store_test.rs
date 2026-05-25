//! Run with: cargo test -p agentverse-memory-pgvector -- --ignored
use agentverse::memory::{Message, MessageRole};
use agentverse_memory_pgvector::PostgresSessionStore;
use agentverse_session::store::SessionStore;

const TEST_DB: &str = "postgresql://localhost/agentverse_test";

#[tokio::test]
#[ignore = "requires live PostgreSQL at localhost/agentverse_test"]
async fn postgres_create_and_get_session() {
    let store = PostgresSessionStore::new(TEST_DB).await.unwrap();
    let session = store.create("pg-user-1").await.unwrap();
    let fetched = store.get(session.id).await.unwrap().unwrap();
    assert_eq!(fetched.user_id, "pg-user-1");
}

#[tokio::test]
#[ignore = "requires live PostgreSQL at localhost/agentverse_test"]
async fn postgres_append_and_load_messages() {
    let store = PostgresSessionStore::new(TEST_DB).await.unwrap();
    let session = store.create("pg-user-2").await.unwrap();
    store
        .append_message(
            session.id,
            Message {
                role: MessageRole::User,
                content: "postgres test".to_string(),
            },
        )
        .await
        .unwrap();
    let messages = store.load_messages(session.id).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "postgres test");
}

#[tokio::test]
#[ignore = "requires live PostgreSQL at localhost/agentverse_test"]
async fn postgres_append_turn_persists_both_in_order() {
    let store = PostgresSessionStore::new(TEST_DB).await.unwrap();
    let session = store.create("pg-user-3").await.unwrap();
    store
        .append_turn(
            session.id,
            Message {
                role: MessageRole::User,
                content: "hello postgres".to_string(),
            },
            Message {
                role: MessageRole::Assistant,
                content: "hi postgres".to_string(),
            },
        )
        .await
        .unwrap();
    let messages = store.load_messages(session.id).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "hello postgres");
    assert_eq!(messages[1].content, "hi postgres");
}
