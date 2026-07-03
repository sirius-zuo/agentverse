use agentverse::model::ConnectionManager;
use agentverse::model::GenerateRequest;
use httpmock::prelude::*;

#[tokio::test]
async fn rate_limited_request_honors_retry_after_instead_of_base_backoff() {
    let server = MockServer::start_async().await;

    let rate_limited = server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(429)
                .header("retry-after", "1")
                .body("slow down");
        })
        .await;

    let cm =
        ConnectionManager::openai(&server.base_url(), "test-model", "key").with_retries(1, 10_000); // huge base delay: only Retry-After can finish this fast

    let start = std::time::Instant::now();
    let result = cm.generate(GenerateRequest::default()).await;
    let elapsed = start.elapsed();

    rate_limited.assert_hits_async(2).await; // initial + 1 retry
    assert!(result.is_err()); // both attempts 429 — error is fine; timing is the assertion
    assert!(
        elapsed >= std::time::Duration::from_secs(1),
        "should have waited Retry-After (1s), waited {elapsed:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "should NOT have used the 10s base backoff, waited {elapsed:?}"
    );
}
