use serde_json::Value;
use std::collections::HashMap;

pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub score: f32,
}

/// BM25 index over tool names and descriptions.
///
/// Parameters follow the standard BM25 defaults (k1=1.5, b=0.75).
pub struct BM25Index {
    docs: Vec<(String, Vec<String>)>,
    df: HashMap<String, usize>,
    k1: f32,
    b: f32,
}

impl BM25Index {
    pub fn new() -> Self {
        Self {
            docs: vec![],
            df: HashMap::new(),
            k1: 1.5,
            b: 0.75,
        }
    }

    pub fn insert(&mut self, id: &str, text: &str) {
        let tokens = tokenize(text);
        let mut seen = std::collections::HashSet::new();
        for t in &tokens {
            if seen.insert(t.clone()) {
                *self.df.entry(t.clone()).or_insert(0) += 1;
            }
        }
        self.docs.push((id.to_string(), tokens));
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<(String, f32)> {
        if self.docs.is_empty() || query.is_empty() {
            return vec![];
        }

        let n = self.docs.len() as f32;
        let avg_len: f32 = self.docs.iter().map(|(_, t)| t.len() as f32).sum::<f32>() / n;
        let q_tokens = tokenize(query);

        let mut scores: Vec<(usize, f32)> = self
            .docs
            .iter()
            .enumerate()
            .map(|(i, (_, tokens))| {
                let dl = tokens.len() as f32;
                let tf_map = term_freq(tokens);
                let score = q_tokens.iter().fold(0.0_f32, |acc, term| {
                    let tf = *tf_map.get(term.as_str()).unwrap_or(&0) as f32;
                    if tf == 0.0 {
                        return acc;
                    }
                    let df = *self.df.get(term.as_str()).unwrap_or(&0) as f32;
                    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                    let tf_norm = (tf * (self.k1 + 1.0))
                        / (tf + self.k1 * (1.0 - self.b + self.b * dl / avg_len));
                    acc + idf * tf_norm
                });
                (i, score)
            })
            .filter(|(_, s)| *s > 0.0)
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(limit);
        scores
            .into_iter()
            .map(|(i, s)| (self.docs[i].0.clone(), s))
            .collect()
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn term_freq(tokens: &[String]) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for t in tokens {
        *map.entry(t.as_str()).or_insert(0) += 1;
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_index_returns_empty() {
        let idx = BM25Index::new();
        assert!(idx.search("calculator", 5).is_empty());
    }

    #[test]
    fn single_doc_matches_query() {
        let mut idx = BM25Index::new();
        idx.insert("calculator", "calculator arithmetic math add subtract");
        let results = idx.search("calculator", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "calculator");
    }

    #[test]
    fn ranks_better_match_first() {
        let mut idx = BM25Index::new();
        idx.insert("calculator", "calculator arithmetic add subtract divide");
        idx.insert("web_search", "web search internet query browser");
        let results = idx.search("arithmetic calculator", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "calculator");
    }

    #[test]
    fn limit_is_respected() {
        let mut idx = BM25Index::new();
        for i in 0..10 {
            idx.insert(&format!("tool_{i}"), &format!("tool number {i} search query"));
        }
        let results = idx.search("tool search", 3);
        assert!(results.len() <= 3);
    }

    #[test]
    fn no_match_returns_empty() {
        let mut idx = BM25Index::new();
        idx.insert("calculator", "arithmetic math numbers");
        let results = idx.search("zxqwerty", 5);
        assert!(results.is_empty());
    }
}
