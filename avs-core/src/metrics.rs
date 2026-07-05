//! OpenTelemetry metrics facade — the ONLY place instruments are created and
//! named. Call sites use the record_* helpers; never create instruments
//! elsewhere. Names follow OTel GenAI semantic conventions where they exist,
//! `agentverse.*` otherwise.
//!
//! Cardinality rule: attributes must be bounded sets (provider names, model
//! names, registered tool names, enum variants). Never user/session IDs or
//! free text; `error.type` is a static variant name, never an error message.
//!
//! Instruments are cached on first use. The OTel global meter does not
//! delegate: install your `SdkMeterProvider` (opentelemetry::global::
//! set_meter_provider) BEFORE the first agent call, or all metrics are
//! silently no-op.

use crate::model::UsageStats;
use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};
use opentelemetry::{global, KeyValue};
use std::sync::OnceLock;
use std::time::Duration;

pub enum RetryReason {
    RateLimited,
    Transport,
}

pub enum ToolOutcome {
    Ok,
    Error,
    HitlIntercepted,
}

pub enum ApprovalEvent {
    Submitted,
    Resolved,
    Expired,
}

pub enum InvokeOutcome {
    Done,
    Interrupted,
    Error,
}

pub enum CacheResult {
    Hit,
    Miss,
}

pub enum SkillRoutingOutcome {
    Matched,
    NoMatch,
}

pub enum PhaseTransitionOutcome {
    Advanced,
    PendingApproval,
}

pub enum HitlTransition {
    Interrupted,
    Resumed,
}

pub enum SessionDeleteReason {
    EndedTtl,
    UserRequest,
}

struct Instruments {
    token_usage: Histogram<u64>,
    llm_duration: Histogram<f64>,
    llm_retries: Counter<u64>,
    circuit_open: Counter<u64>,
    llm_fallback: Counter<u64>,
    tool_calls: Counter<u64>,
    tool_duration: Histogram<f64>,
    approvals: Counter<u64>,
    pending: UpDownCounter<i64>,
    invoke_duration: Histogram<f64>,
    cache_access: Counter<u64>,
    skill_routing: Counter<u64>,
    phase_transitions: Counter<u64>,
    hitl_transitions: Counter<u64>,
    worker_restarts: Counter<u64>,
    session_deleted: Counter<u64>,
    maintenance_backlog: Histogram<u64>,
}

fn instruments() -> &'static Instruments {
    static INSTRUMENTS: OnceLock<Instruments> = OnceLock::new();
    INSTRUMENTS.get_or_init(|| {
        let meter = global::meter("agentverse");
        Instruments {
            token_usage: meter
                .u64_histogram("gen_ai.client.token.usage")
                .with_unit("{token}")
                .with_description("LLM token usage by type")
                .build(),
            llm_duration: meter
                .f64_histogram("gen_ai.client.operation.duration")
                .with_unit("s")
                .with_description("LLM call duration")
                .build(),
            llm_retries: meter
                .u64_counter("agentverse.llm.retries")
                .with_description("LLM retries by reason")
                .build(),
            circuit_open: meter
                .u64_counter("agentverse.llm.circuit_open")
                .with_description("Requests rejected or diverted by an open circuit")
                .build(),
            llm_fallback: meter
                .u64_counter("agentverse.llm.fallback")
                .with_description("Dispatches to the fallback provider")
                .build(),
            tool_calls: meter
                .u64_counter("agentverse.tool.calls")
                .with_description("Tool invocations by outcome")
                .build(),
            tool_duration: meter
                .f64_histogram("agentverse.tool.duration")
                .with_unit("s")
                .with_description("Tool execution duration")
                .build(),
            approvals: meter
                .u64_counter("agentverse.hitl.approvals")
                .with_description("HITL approval lifecycle events")
                .build(),
            pending: meter
                .i64_up_down_counter("agentverse.hitl.pending")
                .with_description("Pending HITL approvals observed by this process")
                .build(),
            invoke_duration: meter
                .f64_histogram("agentverse.agent.invoke.duration")
                .with_unit("s")
                .with_description("Agent::invoke end-to-end duration")
                .build(),
            cache_access: meter
                .u64_counter("agentverse.agent.cache.access")
                .with_description("Layer-1 cache hit/miss on invoke")
                .build(),
            skill_routing: meter
                .u64_counter("agentverse.agent.skill_routing")
                .with_description("Skill router outcomes on first invoke")
                .build(),
            phase_transitions: meter
                .u64_counter("agentverse.agent.phase_transitions")
                .with_description("Skill phase-transition outcomes")
                .build(),
            hitl_transitions: meter
                .u64_counter("agentverse.agent.hitl_transitions")
                .with_description("Agent-level HITL interrupt/resume events")
                .build(),
            worker_restarts: meter
                .u64_counter("agentverse.worker.restarts")
                .with_description("Background worker restarts after panic or unexpected exit")
                .build(),
            session_deleted: meter
                .u64_counter("agentverse.session.deleted")
                .with_description("Sessions deleted, by reason")
                .build(),
            maintenance_backlog: meter
                .u64_histogram("agentverse.session.maintenance_backlog")
                .with_description(
                    "Number of sessions returned by list_sessions_needing_maintenance per call",
                )
                .build(),
        }
    })
}

pub fn record_llm_call(
    provider: &str,
    model: &str,
    usage: Option<&UsageStats>,
    duration: Duration,
    error_type: Option<&'static str>,
) {
    let ins = instruments();
    let base = [
        KeyValue::new("gen_ai.provider.name", provider.to_string()),
        KeyValue::new("gen_ai.request.model", model.to_string()),
    ];
    if let Some(u) = usage {
        for (kind, value) in [
            ("input", u.input_tokens),
            ("output", u.output_tokens),
            ("cache_read", u.cache_read_tokens),
            ("cache_write", u.cache_write_tokens),
        ] {
            let mut attrs = base.to_vec();
            attrs.push(KeyValue::new("gen_ai.token.type", kind));
            ins.token_usage.record(u64::from(value), &attrs);
        }
    }
    let mut attrs = base.to_vec();
    if let Some(e) = error_type {
        attrs.push(KeyValue::new("error.type", e));
    }
    ins.llm_duration.record(duration.as_secs_f64(), &attrs);
}

pub fn record_llm_retry(reason: RetryReason) {
    let reason = match reason {
        RetryReason::RateLimited => "rate_limited",
        RetryReason::Transport => "transport",
    };
    instruments()
        .llm_retries
        .add(1, &[KeyValue::new("reason", reason)]);
}

pub fn record_circuit_open(provider: &str) {
    instruments().circuit_open.add(
        1,
        &[KeyValue::new("gen_ai.provider.name", provider.to_string())],
    );
}

pub fn record_llm_fallback(provider: &str) {
    instruments().llm_fallback.add(
        1,
        &[KeyValue::new("gen_ai.provider.name", provider.to_string())],
    );
}

pub fn record_tool_call(name: &str, duration: Duration, outcome: ToolOutcome) {
    let outcome = match outcome {
        ToolOutcome::Ok => "ok",
        ToolOutcome::Error => "error",
        ToolOutcome::HitlIntercepted => "hitl_intercepted",
    };
    let ins = instruments();
    let attrs = [
        KeyValue::new("tool.name", name.to_string()),
        KeyValue::new("outcome", outcome),
    ];
    ins.tool_calls.add(1, &attrs);
    ins.tool_duration.record(
        duration.as_secs_f64(),
        &[KeyValue::new("tool.name", name.to_string())],
    );
}

pub fn record_approval_event(event: ApprovalEvent) {
    let event = match event {
        ApprovalEvent::Submitted => "submitted",
        ApprovalEvent::Resolved => "resolved",
        ApprovalEvent::Expired => "expired",
    };
    instruments()
        .approvals
        .add(1, &[KeyValue::new("event", event)]);
}

pub fn approvals_pending_delta(delta: i64) {
    instruments().pending.add(delta, &[]);
}

pub fn record_invoke_duration(duration: Duration, outcome: InvokeOutcome) {
    let outcome = match outcome {
        InvokeOutcome::Done => "done",
        InvokeOutcome::Interrupted => "interrupted",
        InvokeOutcome::Error => "error",
    };
    instruments()
        .invoke_duration
        .record(duration.as_secs_f64(), &[KeyValue::new("outcome", outcome)]);
}

pub fn record_cache_access(result: CacheResult) {
    let result = match result {
        CacheResult::Hit => "hit",
        CacheResult::Miss => "miss",
    };
    instruments()
        .cache_access
        .add(1, &[KeyValue::new("result", result)]);
}

pub fn record_skill_routing(outcome: SkillRoutingOutcome) {
    let outcome = match outcome {
        SkillRoutingOutcome::Matched => "matched",
        SkillRoutingOutcome::NoMatch => "no_match",
    };
    instruments()
        .skill_routing
        .add(1, &[KeyValue::new("outcome", outcome)]);
}

pub fn record_phase_transition(outcome: PhaseTransitionOutcome) {
    let outcome = match outcome {
        PhaseTransitionOutcome::Advanced => "advanced",
        PhaseTransitionOutcome::PendingApproval => "pending_approval",
    };
    instruments()
        .phase_transitions
        .add(1, &[KeyValue::new("outcome", outcome)]);
}

pub fn record_hitl_transition(transition: HitlTransition) {
    let transition = match transition {
        HitlTransition::Interrupted => "interrupted",
        HitlTransition::Resumed => "resumed",
    };
    instruments()
        .hitl_transitions
        .add(1, &[KeyValue::new("transition", transition)]);
}

pub fn record_worker_restart(worker: &'static str) {
    instruments()
        .worker_restarts
        .add(1, &[KeyValue::new("worker", worker)]);
}

pub fn record_session_deleted(reason: SessionDeleteReason, count: u64) {
    let reason = match reason {
        SessionDeleteReason::EndedTtl => "ended_ttl",
        SessionDeleteReason::UserRequest => "user_request",
    };
    instruments()
        .session_deleted
        .add(count, &[KeyValue::new("reason", reason)]);
}

pub fn record_maintenance_backlog(count: u64) {
    instruments().maintenance_backlog.record(count, &[]);
}
