use std::fs;

use serde::de::DeserializeOwned;

/// Loads every `*.toml` file in `dir`, deserializing each into `T`.
/// Panics with the file path on any read/parse failure — a broken fixture
/// file is a hard error, not a silently-skipped case.
pub fn load_toml_cases<T: DeserializeOwned>(dir: &str) -> Vec<(String, T)> {
    let mut cases = Vec::new();
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("failed to read fixture dir {dir}: {e}"));
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let case: T = toml::from_str(&content)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        cases.push((name, case));
    }
    assert!(!cases.is_empty(), "no fixture files found in {dir}");
    cases
}

/// A parser regression case: raw ReAct-format LLM output text, and the
/// expected parsed action, compared as a debug-string (avoids requiring
/// `CycleAction` to implement `PartialEq`/`Deserialize` itself).
#[derive(serde::Deserialize)]
pub struct ParserCase {
    pub input: String,
    pub expected_debug: String,
}
