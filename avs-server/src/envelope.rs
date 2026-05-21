use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeKind {
    Invoke,
    Result,
    Error,
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub id: Uuid,
    pub kind: EnvelopeKind,
    pub payload: serde_json::Value,
    pub metadata: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&EnvelopeKind::Invoke).unwrap(),
            "\"invoke\""
        );
        assert_eq!(
            serde_json::to_string(&EnvelopeKind::Pong).unwrap(),
            "\"pong\""
        );
    }

    #[test]
    fn envelope_roundtrip() {
        let env = Envelope {
            id: Uuid::new_v4(),
            kind: EnvelopeKind::Result,
            payload: serde_json::json!({"output": "hello"}),
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, env.id);
        assert_eq!(back.kind, EnvelopeKind::Result);
    }
}
