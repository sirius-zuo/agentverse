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

// ONE test fn: the global meter provider is process-wide, so all assertions
// run sequentially here (see avs-core/tests/metrics_facade_test.rs).
#[tokio::test]
async fn execute_records_tool_call_metrics() {
    let (exporter, provider) = install();
    let registry = agentverse_tools::ToolRegistry::new();
    registry.register(agentverse_tools::Calculator);

    // ok outcome
    let _ = registry
        .execute(
            "calculator",
            serde_json::json!({"operation": "add", "a": 1, "b": 2}),
        )
        .await;
    // error outcome — unknown tool
    let _ = registry.execute("nope", serde_json::Value::Null).await;

    // hitl_intercepted outcome via execute_many_hitl
    struct BlockAll;
    #[async_trait::async_trait]
    impl agentverse::hitl::HitlHook for BlockAll {
        async fn check_tool(&self, _: &str, _: &serde_json::Value) -> Option<(uuid::Uuid, String)> {
            Some((uuid::Uuid::new_v4(), "{}".to_string()))
        }
    }
    let hook: std::sync::Arc<dyn agentverse::hitl::HitlHook> = std::sync::Arc::new(BlockAll);
    let calls = vec![agentverse::ToolCall {
        id: "call_1".to_string(),
        name: "calculator".to_string(),
        args: serde_json::json!({}),
    }];
    let result = registry.execute_many_hitl(calls, &hook).await;
    assert!(result.is_err(), "intercepted call must return Err");

    provider.force_flush().unwrap();
    let names = metric_names(&exporter);
    assert!(
        names.contains(&"agentverse.tool.calls".to_string()),
        "{names:?}"
    );
    assert!(
        names.contains(&"agentverse.tool.duration".to_string()),
        "{names:?}"
    );

    // Confirm a hitl_intercepted datapoint exists on agentverse.tool.calls.
    use opentelemetry::Value;
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    let finished = exporter.get_finished_metrics().unwrap();
    let mut found_hitl_intercepted = false;
    for rm in finished.iter() {
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() != "agentverse.tool.calls" {
                    continue;
                }
                if let AggregatedMetrics::U64(MetricData::Sum(sum)) = m.data() {
                    for dp in sum.data_points() {
                        let is_hitl = dp.attributes().any(|kv| {
                            kv.key.as_str() == "outcome"
                                && kv.value == Value::String("hitl_intercepted".into())
                        });
                        if is_hitl {
                            found_hitl_intercepted = true;
                        }
                    }
                }
            }
        }
    }
    assert!(
        found_hitl_intercepted,
        "expected an agentverse.tool.calls datapoint with outcome=hitl_intercepted"
    );

    // Confirm the "nope" not-found call above was recorded as tool.name="<unknown>",
    // never the raw unregistered name (unbounded cardinality guard).
    let mut found_unknown = false;
    let mut found_raw_name = false;
    for rm in finished.iter() {
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() != "agentverse.tool.calls" {
                    continue;
                }
                if let AggregatedMetrics::U64(MetricData::Sum(sum)) = m.data() {
                    for dp in sum.data_points() {
                        for kv in dp.attributes() {
                            if kv.key.as_str() != "tool.name" {
                                continue;
                            }
                            if kv.value == Value::String("<unknown>".into()) {
                                found_unknown = true;
                            }
                            if kv.value == Value::String("nope".into()) {
                                found_raw_name = true;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        found_unknown,
        "expected a tool.calls datapoint with tool.name=\"<unknown>\" for the not-found call"
    );
    assert!(
        !found_raw_name,
        "raw unregistered tool name \"nope\" must never be recorded as tool.name"
    );
}
