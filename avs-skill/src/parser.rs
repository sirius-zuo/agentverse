use crate::error::SkillError;
use crate::types::Skill;
use serde::Deserialize;
use std::path::Path;

// ── internal frontmatter serde structs ───────────────────────────────────────

#[derive(Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    version: Option<String>,
    tags: Option<Vec<String>>,
    agentverse: Option<AgentverseExt>,
}

#[derive(Deserialize, Default)]
#[allow(dead_code)]
struct AgentverseExt {
    tools: Option<Vec<String>>,
    max_iterations: Option<usize>,
    activation: Option<ActivationExt>,
    // memory_scope and output are parsed-and-dropped for now
    memory_scope: Option<serde_yaml::Value>,
    output: Option<serde_yaml::Value>,
    // HITL fields
    hitl_tools: Option<Vec<String>>,
    phase_gate: Option<bool>,
    checkpoints: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct ActivationExt {
    domains: Option<Vec<String>>,
}

// ── public API ────────────────────────────────────────────────────────────────

/// Parse a SKILL.md file into a `Skill`. `documents` is left empty;
/// `SkillRegistry::compile_context` loads supporting files separately.
pub fn parse_skill_file(path: &Path, content: &str) -> Result<Skill, SkillError> {
    let path_str = path.display().to_string();
    let (frontmatter_src, body) = split_frontmatter(content);

    if frontmatter_src.is_empty() {
        return Err(SkillError::Parse {
            path: path_str,
            message: "missing YAML frontmatter (expected --- delimiters)".into(),
        });
    }

    let fm: SkillFrontmatter =
        serde_yaml::from_str(frontmatter_src).map_err(|e| SkillError::Parse {
            path: path_str.clone(),
            message: e.to_string(),
        })?;

    let avs = fm.agentverse.unwrap_or_default();

    Ok(Skill {
        id: fm.name,
        version: fm.version.unwrap_or_else(|| "0.1.0".to_string()),
        description: fm.description,
        tags: fm.tags.unwrap_or_default(),
        tools: avs.tools.unwrap_or_default(),
        activation_domains: avs.activation.and_then(|a| a.domains).unwrap_or_default(),
        instructions: body.trim().to_string(),
        documents: vec![],
        max_iterations: avs.max_iterations,
        hitl_tools: avs.hitl_tools.unwrap_or_default(),
        phase_gate: avs.phase_gate.unwrap_or(false),
        checkpoints: avs.checkpoints.unwrap_or_default(),
    })
}

/// Split `content` into (frontmatter_src, body).
/// Returns ("", content) if no valid `---` delimiters are found.
/// The opening delimiter must be "---" alone on the first line (followed by \n).
/// The closing delimiter is the next line that is exactly "---" (optional trailing whitespace).
fn split_frontmatter(content: &str) -> (&str, &str) {
    let content = content.trim_start_matches('\n');
    // Opening --- must be exactly "---" followed by a newline, not "----" or "---yaml"
    if !content.starts_with("---\n") {
        return ("", content);
    }
    let after_open = &content[4..]; // skip "---\n"
                                    // Scan for the closing "---" that is alone on its line.
                                    // We look for "\n---" where the remainder of that line is only whitespace.
    let mut pos = 0;
    loop {
        match after_open[pos..].find("\n---") {
            None => return ("", content),
            Some(rel) => {
                let abs = pos + rel;
                let rest = &after_open[abs + 4..]; // text right after "\n---"
                let line_end = rest.find('\n').unwrap_or(rest.len());
                if rest[..line_end].trim().is_empty() {
                    // Valid closing delimiter found
                    let frontmatter = after_open[..abs].trim_start_matches('\n');
                    let body = if line_end < rest.len() {
                        &rest[line_end + 1..]
                    } else {
                        ""
                    };
                    return (frontmatter, body);
                }
                // Not a standalone "---" line — skip past it and keep searching
                pos = abs + 1;
            }
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dummy_path() -> PathBuf {
        PathBuf::from("SKILL.md")
    }

    #[test]
    fn parses_minimal_frontmatter() {
        let content = r#"---
name: test-skill
description: A test skill.
---

# Instructions

Do the thing.
"#;
        let skill = parse_skill_file(&dummy_path(), content).unwrap();
        assert_eq!(skill.id, "test-skill");
        assert_eq!(skill.description, "A test skill.");
        assert_eq!(skill.version, "0.1.0");
        assert!(skill.tools.is_empty());
        assert!(skill.instructions.contains("Do the thing."));
    }

    #[test]
    fn parses_agentverse_extension_fields() {
        let content = r#"---
name: arch-review
description: Review architecture.
version: 1.2.0
tags:
  - architecture

agentverse:
  tools:
    - FileSearch
    - WebSearch
  max_iterations: 15
  activation:
    domains:
      - architecture
---

You are an architect.
"#;
        let skill = parse_skill_file(&dummy_path(), content).unwrap();
        assert_eq!(skill.id, "arch-review");
        assert_eq!(skill.version, "1.2.0");
        assert_eq!(skill.tools, vec!["FileSearch", "WebSearch"]);
        assert_eq!(skill.max_iterations, Some(15));
        assert_eq!(skill.activation_domains, vec!["architecture"]);
        assert_eq!(skill.instructions, "You are an architect.");
    }

    #[test]
    fn returns_error_on_missing_frontmatter() {
        let content = "Just plain markdown, no frontmatter.";
        let err = parse_skill_file(&dummy_path(), content).unwrap_err();
        assert!(matches!(err, SkillError::Parse { .. }));
    }

    #[test]
    fn returns_error_on_missing_required_fields() {
        let content = "---\nversion: 1.0.0\n---\nBody.";
        let err = parse_skill_file(&dummy_path(), content).unwrap_err();
        assert!(matches!(err, SkillError::Parse { .. }));
    }

    #[test]
    fn four_dashes_is_not_valid_frontmatter() {
        // "----\n" starts with "---" but is not a real delimiter
        let content = "----\nname: foo\ndescription: d\n---\nbody";
        let err = parse_skill_file(&dummy_path(), content).unwrap_err();
        assert!(matches!(err, SkillError::Parse { .. }));
    }

    #[test]
    fn body_horizontal_rule_does_not_truncate_instructions() {
        let content =
            "---\nname: foo\ndescription: d\n---\n\nFirst paragraph.\n\n---\n\nSecond paragraph.\n";
        let skill = parse_skill_file(&dummy_path(), content).unwrap();
        assert!(
            skill.instructions.contains("First paragraph."),
            "missing first para"
        );
        assert!(
            skill.instructions.contains("Second paragraph."),
            "missing second para"
        );
    }

    #[test]
    fn parses_hitl_frontmatter_fields() {
        let content = r#"---
name: billing-agent
description: Handles billing.
agentverse:
  tools:
    - ledger_post
    - request_checkpoint
  hitl_tools:
    - ledger_post
  phase_gate: true
  checkpoints:
    - draft_ready
    - before_send
---

You are a billing agent.
"#;
        let skill = parse_skill_file(&dummy_path(), content).unwrap();
        assert_eq!(skill.hitl_tools, vec!["ledger_post"]);
        assert!(skill.phase_gate);
        assert_eq!(skill.checkpoints, vec!["draft_ready", "before_send"]);
    }

    #[test]
    fn hitl_fields_default_to_empty_when_absent() {
        let content = "---\nname: simple\ndescription: Simple.\n---\nInstructions.";
        let skill = parse_skill_file(&dummy_path(), content).unwrap();
        assert!(skill.hitl_tools.is_empty());
        assert!(!skill.phase_gate);
        assert!(skill.checkpoints.is_empty());
    }
}
