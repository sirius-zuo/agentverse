use agentverse::metrics::{record_llm_call, record_tool_call, ToolOutcome};
use std::time::Duration;

#[test]
fn helpers_are_safe_no_ops_without_a_provider() {
    // Must not panic, block, or error.
    record_llm_call("anthropic", "m", None, Duration::from_millis(1), None);
    record_tool_call("t", Duration::from_millis(1), ToolOutcome::Error);
}
