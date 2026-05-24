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
    SqliteSessionStore::new(db_path.to_str().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn create_and_get_session() {
    let store = make_store().await;
    let session = store.create("user-1").await.unwrap();
    let fetched = store.get("user-1", session.id).await.unwrap().unwrap();
    assert_eq!(fetched.user_id, "user-1");
    assert!(matches!(fetched.status, SessionStatus::Active));
}

#[tokio::test]
async fn append_and_load_messages_preserves_order() {
    let store = make_store().await;
    let session = store.create("user-1").await.unwrap();

    store
        .append_message(
            "user-1",
            session.id,
            Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            },
        )
        .await
        .unwrap();
    store
        .append_message(
            "user-1",
            session.id,
            Message {
                role: MessageRole::Assistant,
                content: "hi there".to_string(),
            },
        )
        .await
        .unwrap();
    store
        .append_message(
            "user-1",
            session.id,
            Message {
                role: MessageRole::User,
                content: "how are you".to_string(),
            },
        )
        .await
        .unwrap();

    let messages = store.load_messages("user-1", session.id).await.unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].content, "hello");
    assert_eq!(messages[1].content, "hi there");
    assert_eq!(messages[2].content, "how are you");
}

#[tokio::test]
async fn update_status_marks_completed() {
    let store = make_store().await;
    let session = store.create("user-1").await.unwrap();
    store
        .update_status("user-1", session.id, SessionStatus::Completed)
        .await
        .unwrap();
    let fetched = store.get("user-1", session.id).await.unwrap().unwrap();
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
    let messages = store.load_messages("user-1", session.id).await.unwrap();
    assert!(messages.is_empty());
}

// --- SessionManager tests ---
use agentverse_session::manager::SessionManager;
use std::sync::Arc;

#[tokio::test]
async fn session_manager_create_returns_id() {
    let store = Arc::new(make_store().await);
    let manager = SessionManager::new(store);
    let id = manager.create_session("alice").await.unwrap();
    let session = manager.get_session("alice", id).await.unwrap().unwrap();
    assert_eq!(session.user_id, "alice");
}

#[tokio::test]
async fn session_manager_end_session_marks_completed() {
    let store = Arc::new(make_store().await);
    let manager = SessionManager::new(store);
    let id = manager.create_session("bob").await.unwrap();
    manager.end_session("bob", id).await.unwrap();
    let session = manager.get_session("bob", id).await.unwrap().unwrap();
    assert!(matches!(session.status, SessionStatus::Completed));
}

#[tokio::test]
async fn session_manager_list_sessions_for_user() {
    let store = Arc::new(make_store().await);
    let manager = SessionManager::new(store);
    manager.create_session("carol").await.unwrap();
    manager.create_session("carol").await.unwrap();
    let sessions = manager.list_sessions("carol").await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn user_cannot_access_other_users_session() {
    let store = make_store().await;
    let alice_session = store.create("alice").await.unwrap();

    // append a message as alice
    store
        .append_message(
            "alice",
            alice_session.id,
            Message {
                role: MessageRole::User,
                content: "secret message".to_string(),
            },
        )
        .await
        .unwrap();

    // bob tries to get alice's session — should return None (not found for this user)
    let result = store.get("bob", alice_session.id).await.unwrap();
    assert!(result.is_none(), "bob should not see alice's session");

    // bob tries to load alice's messages — should return NotFound error
    let result = store.load_messages("bob", alice_session.id).await;
    assert!(result.is_err(), "bob should not load alice's messages");

    // bob tries to update alice's session — should return NotFound error
    let result = store
        .update_status("bob", alice_session.id, SessionStatus::Completed)
        .await;
    assert!(result.is_err(), "bob should not update alice's session");
}
