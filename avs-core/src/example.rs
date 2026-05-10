use serde::{Deserialize, Serialize};

/// A few-shot example for prompt templates.
/// Strategy examples use `output`; router examples use `strategy`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Example {
    /// The example input (user request).
    pub input: String,
    /// The example output (agent response). Used by strategy examples.
    #[serde(default)]
    pub output: Option<String>,
    /// The example strategy. Used by router examples.
    #[serde(default)]
    pub strategy: Option<String>,
}
