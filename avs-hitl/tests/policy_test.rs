use agentverse_hitl::HitlPolicy;

#[test]
fn global_blocklist_always_requires_approval() {
    let policy = HitlPolicy::new();
    assert!(policy.requires_tool_approval(None, "exec_command"));
    assert!(policy.requires_tool_approval(Some("any-skill"), "file_delete"));
}

#[test]
fn safe_tool_not_blocked_without_skill_gate() {
    let policy = HitlPolicy::new();
    assert!(!policy.requires_tool_approval(None, "file_read"));
    assert!(!policy.requires_tool_approval(Some("any-skill"), "web_search"));
}

#[test]
fn skill_tool_gate_blocks_for_that_skill_only() {
    let mut policy = HitlPolicy::new();
    policy
        .skill_tool_gates
        .entry("billing".to_string())
        .or_default()
        .insert("stripe_charge".to_string());

    assert!(policy.requires_tool_approval(Some("billing"), "stripe_charge"));
    assert!(!policy.requires_tool_approval(Some("other-skill"), "stripe_charge"));
    assert!(!policy.requires_tool_approval(None, "stripe_charge"));
}

#[test]
fn phase_gate_matches_skill_id() {
    let mut policy = HitlPolicy::new();
    policy
        .skill_phase_gates
        .insert("prepare-journal-entry".to_string());

    assert!(policy.requires_phase_gate("prepare-journal-entry"));
    assert!(!policy.requires_phase_gate("extract-transactions"));
}

#[test]
fn request_checkpoint_is_checkpoint_tool() {
    assert!(HitlPolicy::is_checkpoint_tool("request_checkpoint"));
    assert!(!HitlPolicy::is_checkpoint_tool("web_search"));
}
