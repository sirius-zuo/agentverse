use agentverse::model::{ConnectionManager, GenerateRequest};
use httpmock::prelude::*;
use opentelemetry::global;
use opentelemetry_sdk::metrics::in_memory_exporter::InMemoryMetricExporter;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};

fn install() -> (InMemoryMetricExporter, SdkMeterProvider) {
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(exporter.clone()).build())
        .build();
    global::set_meter_provider(provider.clone());
    (exporter, provider)
}

fn metric_names(exporter: &InMemoryMetricExporter) -> Vec<String> {
    exporter
        .get_finished_metrics()
        .unwrap()
        .iter()
        .flat_map(|rm| rm.scope_metrics())
        .flat_map(|sm| sm.metrics())
        .map(|m| m.name().to_string())
        .collect()
}

#[tokio::test]
async fn successful_generate_records_tokens_and_duration_and_429_records_retry() {
    let (exporter, provider) = install();
    let server = MockServer::start_async().await;

    // First: a 429 then success, so both the retry counter and the success
    // metrics are exercised in one process.
    let rate_limited = server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(429)
                .header("retry-after", "1")
                .body("slow down");
        })
        .await;

    let cm = ConnectionManager::openai(&server.base_url(), "test-model", "key").with_retries(1, 10);
    let _ = cm.generate(GenerateRequest::default()).await; // both attempts 429 → Err
    rate_limited.assert_hits_async(2).await;

    provider.force_flush().unwrap();
    let names = metric_names(&exporter);
    assert!(
        names.contains(&"agentverse.llm.retries".to_string()),
        "retry not recorded: {names:?}"
    );
    assert!(
        names.contains(&"gen_ai.client.operation.duration".to_string()),
        "duration not recorded on failure: {names:?}"
    );
    // token usage must NOT be present — no successful response yet
    assert!(!names.contains(&"gen_ai.client.token.usage".to_string()));
}
