use agentverse_memory::{VectorRecord, VectorStore};
use agentverse_memory_pgvector::PgVectorStore;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Deterministic 1536-dim one-hot embedding: all zeros except `dim` set to 1.0.
/// Cosine distance between two distinct one-hot vectors is exactly 1.0 (orthogonal),
/// and 0.0 between identical ones — giving predictable `relevance` values.
fn one_hot(dim: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; 1536];
    v[dim] = 1.0;
    v
}

fn micros_now() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap()
}

async fn setup(pool: &PgPool) {
    sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
        .execute(pool)
        .await
        .unwrap();
    let migration = include_str!("../src/migration.sql");
    sqlx::raw_sql(migration).execute(pool).await.unwrap();
}

async fn cleanup(pool: &PgPool, user_ids: &[&str]) {
    sqlx::query("DELETE FROM agent_memory WHERE user_id = ANY($1)")
        .bind(user_ids)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn pgvector_store_searches_scoped_to_user_and_scores_relevance() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("TEST_DATABASE_URL not set; skipping Postgres test");
        return;
    };

    let pool = PgPool::connect(&url).await.unwrap();
    setup(&pool).await;
    cleanup(&pool, &["vs-test-alice", "vs-test-bob"]).await;

    let store = PgVectorStore::from_pool(pool.clone());

    let created_a = micros_now();
    let importance_a = 0.9f32;

    store
        .store(VectorRecord {
            user_id: "vs-test-alice".to_string(),
            content: "alice record A".to_string(),
            embedding: one_hot(0),
            importance: importance_a,
            created_at: created_a,
        })
        .await
        .unwrap();

    store
        .store(VectorRecord {
            user_id: "vs-test-alice".to_string(),
            content: "alice record B".to_string(),
            embedding: one_hot(1),
            importance: 0.3,
            created_at: micros_now(),
        })
        .await
        .unwrap();

    store
        .store(VectorRecord {
            user_id: "vs-test-alice".to_string(),
            content: "alice record C".to_string(),
            embedding: one_hot(2),
            importance: 0.4,
            created_at: micros_now(),
        })
        .await
        .unwrap();

    // Bob has a record embedded in the *same* direction as alice's record A —
    // if user-scoping were broken, this would be the top hit for alice's search.
    store
        .store(VectorRecord {
            user_id: "vs-test-bob".to_string(),
            content: "bob record".to_string(),
            embedding: one_hot(0),
            importance: 0.9,
            created_at: micros_now(),
        })
        .await
        .unwrap();

    let hits = store.search("vs-test-alice", &one_hot(0), 3).await.unwrap();

    assert_eq!(hits.len(), 3);
    assert!(
        hits.iter().all(|h| h.content.starts_with("alice")),
        "only alice's rows should be returned, got: {hits:?}"
    );

    let top = &hits[0];
    assert_eq!(top.content, "alice record A");
    assert!(
        top.relevance > 0.0 && top.relevance <= 1.0,
        "relevance {} out of (0,1]",
        top.relevance
    );
    assert!(
        (top.relevance - 1.0).abs() < 1e-4,
        "identical embedding should score relevance ~1.0, got {}",
        top.relevance
    );
    assert!((top.importance - importance_a).abs() < 1e-6);
    assert_eq!(top.created_at, created_a);

    for h in &hits[1..] {
        assert!(h.relevance > 0.0 && h.relevance <= 1.0);
    }

    cleanup(&pool, &["vs-test-alice", "vs-test-bob"]).await;
}
