use crate::error::SkillError;
use crate::mode::SkillMode;
use crate::parser::parse_skill_file;
use crate::types::{Skill, SkillContext, SkillId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct SkillRegistry {
    /// name → (skill, package_directory)
    skills: HashMap<SkillId, (Skill, PathBuf)>,
}

impl SkillRegistry {
    /// Load skills from `<skills_dir>/system/` then `<skills_dir>/user/`.
    /// User skills shadow system skills with the same `name`.
    pub fn load(skills_dir: &Path) -> Result<Self, SkillError> {
        let mut skills: HashMap<SkillId, (Skill, PathBuf)> = HashMap::new();

        let system_dir = skills_dir.join("system");
        if system_dir.exists() {
            load_dir(&system_dir, &mut skills)?;
        }

        // User skills loaded second — they overwrite system skills with same name.
        let user_dir = skills_dir.join("user");
        if user_dir.exists() {
            load_dir(&user_dir, &mut skills)?;
        }

        Ok(Self { skills })
    }

    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.skills.get(id).map(|(s, _)| s)
    }

    /// Return skills eligible for routing under the given mode.
    pub fn eligible(&self, mode: &SkillMode) -> Vec<&Skill> {
        match mode {
            SkillMode::Open => self.skills.values().map(|(s, _)| s).collect(),
            SkillMode::Constrained(ids) => ids
                .iter()
                .filter_map(|id| self.skills.get(id).map(|(s, _)| s))
                .collect(),
        }
    }

    /// Return the filesystem directory for the given skill package, if loaded.
    pub fn skill_dir(&self, id: &str) -> Option<&std::path::Path> {
        self.skills.get(id).map(|(_, dir)| dir.as_path())
    }

    /// Compile a `SkillContext` for the given skill id.
    /// Documents were loaded eagerly at registry creation; this clones them.
    pub fn compile_context(&self, id: &str) -> Result<SkillContext, SkillError> {
        let (skill, _pkg_dir) = self
            .skills
            .get(id)
            .ok_or_else(|| SkillError::NotFound(id.to_string()))?;

        Ok(SkillContext {
            skill_id: skill.id.clone(),
            instructions: skill.instructions.clone(),
            documents: skill.documents.clone(),
            tools: skill.tools.clone(),
            max_iterations: skill.max_iterations,
        })
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn load_dir(dir: &Path, skills: &mut HashMap<SkillId, (Skill, PathBuf)>) -> Result<(), SkillError> {
    let entries = std::fs::read_dir(dir).map_err(SkillError::Io)?;
    for entry in entries {
        let entry = entry.map_err(SkillError::Io)?;
        let pkg_dir = entry.path();
        if !pkg_dir.is_dir() {
            continue;
        }
        let skill_md = pkg_dir.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&skill_md).map_err(SkillError::Io)?;
        let mut skill = parse_skill_file(&skill_md, &content)?;

        // Eager-load supporting documents
        skill.documents = collect_supporting_files(&pkg_dir)?;

        let id = skill.id.clone();
        skills.insert(id, (skill, pkg_dir));
    }
    Ok(())
}

/// Recursively collect the text contents of all files in `dir` except `SKILL.md`.
fn collect_supporting_files(dir: &Path) -> Result<Vec<String>, SkillError> {
    let mut docs = Vec::new();
    visit_dir(dir, &mut docs)?;
    Ok(docs)
}

fn visit_dir(dir: &Path, docs: &mut Vec<String>) -> Result<(), SkillError> {
    let entries = std::fs::read_dir(dir).map_err(SkillError::Io)?;
    for entry in entries {
        let entry = entry.map_err(SkillError::Io)?;
        let path = entry.path();
        if path.is_dir() {
            visit_dir(&path, docs)?;
        } else if path.file_name().and_then(|n| n.to_str()) != Some("SKILL.md") {
            let content = std::fs::read_to_string(&path).map_err(SkillError::Io)?;
            docs.push(content);
        }
    }
    Ok(())
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_skill_pkg(base: &Path, subdir: &str, name: &str, tools: &[&str], body: &str) {
        let pkg = base.join(subdir).join(name);
        fs::create_dir_all(&pkg).unwrap();
        let tools_yaml = tools
            .iter()
            .map(|t| format!("    - {t}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tools_block = if tools.is_empty() {
            String::new()
        } else {
            format!("agentverse:\n  tools:\n{tools_yaml}\n")
        };
        let content = format!(
            "---\nname: {name}\ndescription: {name} description.\n{tools_block}---\n\n{body}\n"
        );
        fs::write(pkg.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn loads_system_skill() {
        let dir = tempfile::tempdir().unwrap();
        make_skill_pkg(
            dir.path(),
            "system",
            "code-review",
            &["FileSearch"],
            "Review code.",
        );
        let reg = SkillRegistry::load(dir.path()).unwrap();
        let skill = reg.get("code-review").unwrap();
        assert_eq!(skill.id, "code-review");
        assert_eq!(skill.tools, vec!["FileSearch"]);
    }

    #[test]
    fn user_skill_shadows_system_skill() {
        let dir = tempfile::tempdir().unwrap();
        make_skill_pkg(dir.path(), "system", "planning", &[], "System planning.");
        make_skill_pkg(
            dir.path(),
            "user",
            "planning",
            &["ShellTool"],
            "User planning.",
        );
        let reg = SkillRegistry::load(dir.path()).unwrap();
        let skill = reg.get("planning").unwrap();
        // User version wins
        assert_eq!(skill.instructions, "User planning.");
        assert_eq!(skill.tools, vec!["ShellTool"]);
    }

    #[test]
    fn compile_context_loads_supporting_files() {
        let dir = tempfile::tempdir().unwrap();
        make_skill_pkg(dir.path(), "system", "arch-review", &[], "Instructions.");
        // Add a supporting file
        let pkg_dir = dir.path().join("system").join("arch-review");
        std::fs::write(
            pkg_dir.join("principles.md"),
            "## Principles\nPrefer simple.",
        )
        .unwrap();

        let reg = SkillRegistry::load(dir.path()).unwrap();
        let ctx = reg.compile_context("arch-review").unwrap();
        assert_eq!(ctx.instructions, "Instructions.");
        assert_eq!(ctx.documents.len(), 1);
        assert!(ctx.documents[0].contains("Prefer simple."));
    }

    #[test]
    fn compile_context_returns_not_found_for_unknown_skill() {
        let dir = tempfile::tempdir().unwrap();
        let reg = SkillRegistry::load(dir.path()).unwrap();
        let err = reg.compile_context("nonexistent").unwrap_err();
        assert!(matches!(err, SkillError::NotFound(_)));
    }

    #[test]
    fn empty_skills_dir_loads_without_error() {
        let dir = tempfile::tempdir().unwrap();
        let reg = SkillRegistry::load(dir.path()).unwrap();
        assert!(reg.get("anything").is_none());
    }

    #[test]
    fn load_returns_self_not_arc() {
        let dir = tempfile::tempdir().unwrap();
        make_skill_pkg(dir.path(), "system", "review", &[], "Review.");
        // If this compiles and runs, load returns Self (not Arc<Self>)
        let reg: SkillRegistry = SkillRegistry::load(dir.path()).unwrap();
        assert!(reg.get("review").is_some());
    }

    #[test]
    fn eligible_open_returns_all_skills() {
        use crate::mode::SkillMode;
        let dir = tempfile::tempdir().unwrap();
        make_skill_pkg(dir.path(), "system", "skill-a", &[], "A.");
        make_skill_pkg(dir.path(), "system", "skill-b", &[], "B.");
        let reg = SkillRegistry::load(dir.path()).unwrap();
        let eligible = reg.eligible(&SkillMode::Open);
        assert_eq!(eligible.len(), 2);
    }

    #[test]
    fn eligible_constrained_filters_to_listed_ids() {
        use crate::mode::SkillMode;
        let dir = tempfile::tempdir().unwrap();
        make_skill_pkg(dir.path(), "system", "skill-a", &[], "A.");
        make_skill_pkg(dir.path(), "system", "skill-b", &[], "B.");
        make_skill_pkg(dir.path(), "system", "skill-c", &[], "C.");
        let reg = SkillRegistry::load(dir.path()).unwrap();
        let mode = SkillMode::Constrained(vec!["skill-a".into(), "skill-c".into()]);
        let eligible = reg.eligible(&mode);
        let ids: Vec<&str> = eligible.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"skill-a"));
        assert!(ids.contains(&"skill-c"));
        assert!(!ids.contains(&"skill-b"));
    }

    #[test]
    fn eligible_constrained_with_unknown_id_skips_silently() {
        use crate::mode::SkillMode;
        let dir = tempfile::tempdir().unwrap();
        make_skill_pkg(dir.path(), "system", "skill-a", &[], "A.");
        let reg = SkillRegistry::load(dir.path()).unwrap();
        let mode = SkillMode::Constrained(vec!["skill-a".into(), "nonexistent".into()]);
        let eligible = reg.eligible(&mode);
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].id, "skill-a");
    }

    #[test]
    fn skill_dir_returns_path_for_known_skill() {
        let dir = tempfile::tempdir().unwrap();
        make_skill_pkg(dir.path(), "system", "my-skill", &[], "Do things.");
        let reg = SkillRegistry::load(dir.path()).unwrap();
        let skill_dir = reg.skill_dir("my-skill").unwrap();
        assert!(skill_dir.ends_with("my-skill"));
    }

    #[test]
    fn skill_dir_returns_none_for_unknown_skill() {
        let dir = tempfile::tempdir().unwrap();
        let reg = SkillRegistry::load(dir.path()).unwrap();
        assert!(reg.skill_dir("nonexistent").is_none());
    }
}
