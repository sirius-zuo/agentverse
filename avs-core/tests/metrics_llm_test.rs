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

// ONE test fn: the global meter provider is process-wide and instruments are
// cached on first use, so all assertions here must run sequentially in a
// single test (see metrics_facade_test.rs / metrics_tools_test.rs).
#[tokio::test]
async fn generate_records_metrics_on_retry_and_body_read_failure() {
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

    // Second: a body-read failure (the connection drops mid-body, after a
    // well-formed status line and headers) must still record
    // gen_ai.client.operation.duration with error.type = "api_error" — this
    // path used to `?`-propagate past every instrumented recording point.
    // httpmock always sends a well-formed body, so this uses a raw TCP
    // listener that advertises more bytes (Content-Length) than it actually
    // sends, then closes the connection — this reliably makes reqwest's
    // `resp.text().await` fail with an incomplete-body error while
    // `send().await` itself still succeeds (status + headers parse fine).
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            // Best-effort drain of whatever the client already sent; we don't
            // need to fully parse it to produce a broken response.
            let mut buf = [0u8; 4096];
            let _ =
                tokio::time::timeout(std::time::Duration::from_millis(200), socket.read(&mut buf))
                    .await;
            // Claims 1000 body bytes but sends 5, then closes — an
            // incomplete message that fails during body read, not during
            // status/header parsing.
            let response =
                b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nshort";
            let _ = socket.write_all(response).await;
            let _ = socket.shutdown().await;
        }
    });

    let cm2 = ConnectionManager::openai(&format!("http://{addr}"), "test-model", "key");
    let result = cm2.generate(GenerateRequest::default()).await;
    assert!(result.is_err(), "body read failure must propagate as Err");

    provider.force_flush().unwrap();
    let finished = exporter.get_finished_metrics().unwrap();

    use opentelemetry::Value;
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    let mut found_api_error_duration = false;
    for rm in finished.iter() {
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() != "gen_ai.client.operation.duration" {
                    continue;
                }
                if let AggregatedMetrics::F64(MetricData::Histogram(hist)) = m.data() {
                    for dp in hist.data_points() {
                        let is_api_error = dp.attributes().any(|kv| {
                            kv.key.as_str() == "error.type"
                                && kv.value == Value::String("api_error".into())
                        });
                        if is_api_error {
                            found_api_error_duration = true;
                        }
                    }
                }
            }
        }
    }
    assert!(
        found_api_error_duration,
        "expected gen_ai.client.operation.duration recorded with error.type=api_error \
         on a body-read failure"
    );
}
