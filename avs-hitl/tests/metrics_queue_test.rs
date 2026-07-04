use agentverse_hitl::{
    ApprovalDecision, ApprovalQueue, ApprovalRequest, InMemoryQueue, InterruptKind, SqliteQueue,
};
use opentelemetry::global;
use opentelemetry_sdk::metrics::in_memory_exporter::InMemoryMetricExporter;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use uuid::Uuid;

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

fn make_request(session_id: Uuid) -> ApprovalRequest {
    ApprovalRequest::new(
        session_id,
        InterruptKind::ToolApproval {
            tool_name: "exec_command".to_string(),
            args: serde_json::json!({}),
        },
    )
}

#[tokio::test]
async fn queue_lifecycle_records_metrics_for_both_backends() {
    let (exporter, provider) = install();

    // SqliteQueue: submit, resolve, sweep_expired
    let sqlite_q = SqliteQueue::new("sqlite::memory:").await.unwrap();
    let id = sqlite_q.submit(make_request(Uuid::new_v4())).await.unwrap();
    sqlite_q
        .resolve(id, ApprovalDecision::Approved)
        .await
        .unwrap();
    let mut expiring = make_request(Uuid::new_v4());
    expiring.expires_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    sqlite_q.submit(expiring).await.unwrap();
    let swept = sqlite_q.sweep_expired().await.unwrap();
    assert_eq!(swept, 1);

    // InMemoryQueue: submit, resolve
    let mem_q = InMemoryQueue::new();
    let id = mem_q.submit(make_request(Uuid::new_v4())).await.unwrap();
    mem_q.resolve(id, ApprovalDecision::Approved).await.unwrap();

    provider.force_flush().unwrap();
    let names = metric_names(&exporter);
    assert!(
        names.contains(&"agentverse.hitl.approvals".to_string()),
        "{names:?}"
    );
    assert!(
        names.contains(&"agentverse.hitl.pending".to_string()),
        "{names:?}"
    );
}
