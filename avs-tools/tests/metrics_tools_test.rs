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
}
