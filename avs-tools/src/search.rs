use serde_json::Value;
use std::collections::HashMap;

pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub score: f32,
}

pub struct BM25Index {
    docs: Vec<(String, Vec<String>)>,
    df: HashMap<String, usize>,
}

impl BM25Index {
    pub fn new() -> Self {
        Self {
            docs: vec![],
            df: HashMap::new(),
        }
    }

    pub fn insert(&mut self, id: &str, text: &str) {
        let tokens = tokenize(text);
        for t in &tokens {
            *self.df.entry(t.clone()).or_insert(0) += 1;
        }
        self.docs.push((id.to_string(), tokens));
    }

    pub fn search(&self, _query: &str, _limit: usize) -> Vec<(String, f32)> {
        vec![]
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}
