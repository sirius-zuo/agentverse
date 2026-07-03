use agentverse_session::SqliteSessionMemory;
use agentverse_test_utils::session_conformance::run_conformance_suite;

#[tokio::test]
async fn sqlite_session_store_conforms() {
    let store = SqliteSessionMemory::new("sqlite::memory:").await.unwrap();
    run_conformance_suite(&store).await;
}
