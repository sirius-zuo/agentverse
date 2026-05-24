use agentverse::memory::{Message, MessageRole};
use agentverse_session::session::SessionStatus;
use agentverse_session::sqlite::SqliteSessionStore;
use agentverse_session::store::SessionStore;
use tempfile::tempdir;

async fn make_store() -> SqliteSessionStore {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    // keep dir alive for the test duration — leak it intentionally
    std::mem::forget(dir);
    SqliteSessionStore::new(db_path.to_str().unwrap()).await.unwrap()
}

#[tokio::test]
async fn create_and_get_session() {
    let store = make_store().await;
    let session = store.create("user-1").await.unwrap();
    let fetched = store.get(session.id).await.unwrap().unwrap();
    assert_eq!(fetched.user_id, "user-1");
    assert!(matches!(fetched.status, SessionStatus::Active));
}

#[tokio::test]
async fn append_and_load_messages_preserves_order() {
    let store = make_store().await;
    let session = store.create("user-1").await.unwrap();

    store.append_message(session.id, Message { role: MessageRole::User, content: "hello".to_string() }).await.unwrap();
    store.append_message(session.id, Message { role: MessageRole::Assistant, content: "hi there".to_string() }).await.unwrap();
    store.append_message(session.id, Message { role: MessageRole::User, content: "how are you".to_string() }).await.unwrap();

    let messages = store.load_messages(session.id).await.unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].content, "hello");
    assert_eq!(messages[1].content, "hi there");
    assert_eq!(messages[2].content, "how are you");
}

#[tokio::test]
async fn update_status_marks_completed() {
    let store = make_store().await;
    let session = store.create("user-1").await.unwrap();
    store.update_status(session.id, SessionStatus::Completed).await.unwrap();
    let fetched = store.get(session.id).await.unwrap().unwrap();
    assert!(matches!(fetched.status, SessionStatus::Completed));
}

#[tokio::test]
async fn list_by_user_returns_all_sessions() {
    let store = make_store().await;
    store.create("alice").await.unwrap();
    store.create("alice").await.unwrap();
    store.create("bob").await.unwrap();
    let alice_sessions = store.list_by_user("alice").await.unwrap();
    assert_eq!(alice_sessions.len(), 2);
}

#[tokio::test]
async fn load_messages_empty_for_new_session() {
    let store = make_store().await;
    let session = store.create("user-1").await.unwrap();
    let messages = store.load_messages(session.id).await.unwrap();
    assert!(messages.is_empty());
}
