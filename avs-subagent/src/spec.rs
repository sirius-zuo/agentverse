use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentSpec {
    pub name: String,
    pub objective: String,
    pub system_prompt: Option<String>,
    pub model: Option<ModelOverride>,
    pub allowed_tools: Vec<String>,
    pub budget: Budget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    pub max_steps: usize,
    pub max_tokens: u32,
    #[serde(with = "duration_secs")]
    pub timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ModelOverride {
    Alias(String),
    Id(String),
}

mod duration_secs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;
    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        d.as_secs().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_secs(u64::deserialize(d)?))
    }
}
