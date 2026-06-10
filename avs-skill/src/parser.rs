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
struct AgentverseExt {
    tools: Option<Vec<String>>,
    max_iterations: Option<usize>,
    activation: Option<ActivationExt>,
    // memory_scope and output are parsed-and-dropped for now
    memory_scope: Option<serde_yaml::Value>,
    output: Option<serde_yaml::Value>,
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
        activation_domains: avs
            .activation
            .and_then(|a| a.domains)
            .unwrap_or_default(),
        instructions: body.trim().to_string(),
        documents: vec![],
        max_iterations: avs.max_iterations,
    })
}

/// Split `content` into (frontmatter_src, body).
/// Returns ("", content) if no `---` delimiters are found.
fn split_frontmatter(content: &str) -> (&str, &str) {
    let content = content.trim_start_matches('\n');
    if !content.starts_with("---") {
        return ("", content);
    }
    // Skip the opening ---
    let after_open = &content[3..];
    // Find the closing --- (must be on its own line)
    if let Some(close_pos) = after_open.find("\n---") {
        let frontmatter = after_open[..close_pos].trim_start_matches('\n');
        let after_close = &after_open[close_pos + 4..]; // skip \n---
        let body = after_close.strip_prefix('\n').unwrap_or(after_close);
        (frontmatter, body)
    } else {
        ("", content)
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
}
