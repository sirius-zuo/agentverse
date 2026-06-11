use agentverse_subagent::load_skill_subagent_spec;
use std::fs;
use std::time::Duration;

#[test]
fn load_skill_subagent_spec_parses_yaml() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("subagent.yaml"),
        r#"
name: code-reviewer
objective: "Review code for issues"
model:
  type: alias
  value: haiku
allowed_tools:
  - read_file
  - grep
budget:
  max_steps: 10
  max_tokens: 10000
  timeout: 60
"#,
    )
    .unwrap();

    let spec = load_skill_subagent_spec(dir.path()).unwrap().unwrap();
    assert_eq!(spec.name, "code-reviewer");
    assert_eq!(spec.allowed_tools, vec!["read_file", "grep"]);
    assert_eq!(spec.budget.max_steps, 10);
    assert_eq!(spec.budget.max_tokens, 10000);
    assert_eq!(spec.budget.timeout, Duration::from_secs(60));
}

#[test]
fn load_skill_subagent_spec_returns_none_when_no_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let result = load_skill_subagent_spec(dir.path()).unwrap();
    assert!(result.is_none());
}

#[test]
fn load_skill_subagent_spec_handles_plain_string_model() {
    use agentverse_subagent::ModelOverride;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("subagent.yaml"),
        r#"
name: summarizer
objective: "Summarize the document"
model: haiku
allowed_tools: []
budget:
  max_steps: 5
  max_tokens: 2000
  timeout: 30
"#,
    )
    .unwrap();

    let spec = agentverse_subagent::load_skill_subagent_spec(dir.path())
        .unwrap()
        .unwrap();
    assert_eq!(spec.name, "summarizer");
    assert!(matches!(spec.model, Some(ModelOverride::Alias(ref s)) if s == "haiku"));
}
