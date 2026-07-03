use agentverse_memory_pgvector::PostgresSessionMemory;
use agentverse_test_utils::session_conformance::run_conformance_suite;

#[tokio::test]
async fn postgres_session_store_conforms() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("TEST_DATABASE_URL not set; skipping Postgres conformance test");
        return;
    };
    let store = PostgresSessionMemory::new(&url).await.unwrap();
    run_conformance_suite(&store).await;
}
