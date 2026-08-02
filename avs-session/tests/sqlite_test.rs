use agentverse::memory::{Message, MessageRole};
use agentverse_session::session::SessionStatus;
use agentverse_session::sqlite::SqliteSessionMemory;
use agentverse_session::store::SessionMemory;
use tempfile::tempdir;

async fn make_store() -> SqliteSessionMemory {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    std::mem::forget(dir);
    SqliteSessionMemory::new(db_path.to_str().unwrap())
        .await
        .unwrap()
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
async fn get_by_session_id_returns_session_without_user_argument() {
    let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
    let session = store.create("alice").await.unwrap();
    let loaded = store.get(session.id).await.unwrap().unwrap();
    assert_eq!(loaded.id, session.id);
    assert_eq!(loaded.user_id, "alice");
}

#[tokio::test]
async fn append_and_load_messages_preserves_order() {
    let store = make_store().await;
    let session = store.create("user-1").await.unwrap();

    store
        .append_message(session.id, Message::text(MessageRole::User, "hello"))
        .await
        .unwrap();
    store
        .append_message(
            session.id,
            Message::text(MessageRole::Assistant, "hi there"),
        )
        .await
        .unwrap();
    store
        .append_message(session.id, Message::text(MessageRole::User, "how are you"))
        .await
        .unwrap();

    let messages = store.load_messages(session.id).await.unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].as_text(), "hello");
    assert_eq!(messages[1].as_text(), "hi there");
    assert_eq!(messages[2].as_text(), "how are you");
}

#[tokio::test]
async fn append_turn_persists_both_messages_in_order() {
    let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
    let session = store.create("alice").await.unwrap();

    store
        .append_turn(
            session.id,
            Message::text(MessageRole::User, "hello"),
            Message::text(MessageRole::Assistant, "hi"),
        )
        .await
        .unwrap();

    let messages = store.load_messages(session.id).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert!(matches!(messages[0].role, MessageRole::User));
    assert_eq!(messages[0].as_text(), "hello");
    assert!(matches!(messages[1].role, MessageRole::Assistant));
    assert_eq!(messages[1].as_text(), "hi");
}

#[tokio::test]
async fn update_status_marks_completed() {
    let store = make_store().await;
    let session = store.create("user-1").await.unwrap();
    store
        .update_status(session.id, SessionStatus::Completed)
        .await
        .unwrap();
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

// --- SessionManager tests ---
use agentverse_session::manager::SessionManager;
use std::sync::Arc;

#[tokio::test]
async fn session_manager_create_returns_id() {
    let store = Arc::new(make_store().await);
    let manager = SessionManager::new(store);
    let id = manager.create_session("alice").await.unwrap();
    let session = manager.get_session(id).await.unwrap().unwrap();
    assert_eq!(session.user_id, "alice");
}

#[tokio::test]
async fn session_manager_end_session_marks_completed() {
    let store = Arc::new(make_store().await);
    let manager = SessionManager::new(store);
    let id = manager.create_session("bob").await.unwrap();
    manager.end_session(id).await.unwrap();
    let session = manager.get_session(id).await.unwrap().unwrap();
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
    let store: Arc<dyn SessionMemory> = Arc::new(make_store().await);
    let alice_session = store.create("alice").await.unwrap();

    store
        .append_message(
            alice_session.id,
            Message::text(MessageRole::User, "secret message"),
        )
        .await
        .unwrap();

    // get() no longer filters by user_id; ownership is checked via assert_owner
    let result = store.get(alice_session.id).await.unwrap();
    assert!(result.is_some(), "session should be found by id");
    assert_eq!(result.unwrap().user_id, "alice");

    // bob tries to assert ownership — should fail
    let manager = SessionManager::new(Arc::clone(&store));
    let err = manager
        .assert_owner("bob", alice_session.id)
        .await
        .unwrap_err();
    assert!(
        matches!(err, agentverse_session::SessionMemoryError::NotFound(id) if id == alice_session.id),
        "bob should not own alice's session"
    );
}
