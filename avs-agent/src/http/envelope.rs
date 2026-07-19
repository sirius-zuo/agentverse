use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeKind {
    Invoke,
    Result,
    Error,
    Suspended,
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

/// Payload of an `EnvelopeKind::Suspended` envelope (agent → aether).
/// Byte-identical to `aether_core::resume::SuspendPayload`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuspendPayload {
    pub session_id: String,
    pub approval_id: String,
    pub kind: String,
    pub prompt: String,
}

/// Human decision as relayed by aether on `POST /aether/resume`.
/// Internally tagged; byte-identical to `aether_core::resume::ApprovalDecision`.
/// NOTE: distinct from `agentverse_hitl::ApprovalDecision`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AetherApprovalDecision {
    Approved,
    Rejected { reason: Option<String> },
    Modified { payload: serde_json::Value },
}

/// Body of a `POST /aether/resume` request (aether → agent).
/// Byte-identical to `aether_core::resume::ResumeRequest`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeRequest {
    pub session_id: String,
    pub approval_id: String,
    pub decision: AetherApprovalDecision,
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

    #[test]
    fn suspended_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&EnvelopeKind::Suspended).unwrap(),
            "\"suspended\""
        );
    }

    // Round-trip a struct against its fixture by comparing parsed Values
    // (whitespace-insensitive). Drift in either direction fails the test.
    fn assert_fixture_roundtrip<T>(fixture: &str)
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let expected: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let parsed: T = serde_json::from_str(fixture).unwrap();
        let actual = serde_json::to_value(&parsed).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn suspend_payload_matches_fixture() {
        assert_fixture_roundtrip::<SuspendPayload>(include_str!(
            "../../tests/fixtures/suspend_payload.json"
        ));
    }

    #[test]
    fn resume_request_variants_match_fixtures() {
        assert_fixture_roundtrip::<ResumeRequest>(include_str!(
            "../../tests/fixtures/resume_request_approved.json"
        ));
        assert_fixture_roundtrip::<ResumeRequest>(include_str!(
            "../../tests/fixtures/resume_request_rejected.json"
        ));
        assert_fixture_roundtrip::<ResumeRequest>(include_str!(
            "../../tests/fixtures/resume_request_modified.json"
        ));
    }
}
