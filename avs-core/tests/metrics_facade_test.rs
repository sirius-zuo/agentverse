use agentverse::metrics::{
    approvals_pending_delta, record_approval_event, record_circuit_open, record_llm_call,
    record_llm_fallback, record_llm_retry, record_tool_call, ApprovalEvent, RetryReason,
    ToolOutcome,
};
use agentverse::model::UsageStats;
use opentelemetry::global;
use opentelemetry_sdk::metrics::in_memory_exporter::InMemoryMetricExporter;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use std::time::Duration;

// ONE test fn: the global meter provider is process-wide and instruments are
// cached on first use, so all facade assertions run sequentially here.
#[test]
fn facade_records_all_instruments_with_expected_names_and_attributes() {
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(exporter.clone()).build())
        .build();
    global::set_meter_provider(provider.clone());

    let usage = UsageStats {
        input_tokens: 100,
        output_tokens: 40,
        cache_write_tokens: 10,
        cache_read_tokens: 25,
    };
    record_llm_call(
        "anthropic",
        "claude-sonnet-5",
        Some(&usage),
        Duration::from_millis(1500),
        None,
    );
    record_llm_call(
        "openai",
        "gpt-4o",
        None,
        Duration::from_millis(200),
        Some("api_error"),
    );
    record_llm_retry(RetryReason::RateLimited);
    record_circuit_open("anthropic");
    record_llm_fallback("anthropic");
    record_tool_call("calculator", Duration::from_millis(3), ToolOutcome::Ok);
    record_tool_call(
        "web_search",
        Duration::from_millis(90),
        ToolOutcome::HitlIntercepted,
    );
    record_approval_event(ApprovalEvent::Submitted);
    approvals_pending_delta(1);
    approvals_pending_delta(-1);
    agentverse::metrics::record_invoke_duration(
        std::time::Duration::from_millis(50),
        agentverse::metrics::InvokeOutcome::Done,
    );
    agentverse::metrics::record_cache_access(agentverse::metrics::CacheResult::Hit);
    agentverse::metrics::record_skill_routing(agentverse::metrics::SkillRoutingOutcome::Matched);
    agentverse::metrics::record_phase_transition(
        agentverse::metrics::PhaseTransitionOutcome::Advanced,
    );

    provider.force_flush().unwrap();
    let finished = exporter.get_finished_metrics().unwrap();

    let names: Vec<String> = finished
        .iter()
        .flat_map(|rm| rm.scope_metrics())
        .flat_map(|sm| sm.metrics())
        .map(|m| m.name().to_string())
        .collect();

    for expected in [
        "gen_ai.client.token.usage",
        "gen_ai.client.operation.duration",
        "agentverse.llm.retries",
        "agentverse.llm.circuit_open",
        "agentverse.llm.fallback",
        "agentverse.tool.calls",
        "agentverse.tool.duration",
        "agentverse.hitl.approvals",
        "agentverse.hitl.pending",
        "agentverse.agent.invoke.duration",
        "agentverse.agent.cache.access",
        "agentverse.agent.skill_routing",
        "agentverse.agent.phase_transitions",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "missing metric {expected}, got {names:?}"
        );
    }

    // Confirm one token-usage datapoint carries gen_ai.token.type = "input" with value 100.
    use opentelemetry::Value;
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};

    let mut found_input_datapoint = false;
    for rm in finished.iter() {
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() != "gen_ai.client.token.usage" {
                    continue;
                }
                if let AggregatedMetrics::U64(MetricData::Histogram(hist)) = m.data() {
                    for dp in hist.data_points() {
                        let is_input = dp.attributes().any(|kv| {
                            kv.key.as_str() == "gen_ai.token.type"
                                && kv.value == Value::String("input".into())
                        });
                        if is_input && dp.sum() == 100 {
                            found_input_datapoint = true;
                        }
                    }
                }
            }
        }
    }
    assert!(
        found_input_datapoint,
        "expected a gen_ai.client.token.usage datapoint with gen_ai.token.type=input and sum=100"
    );
}
