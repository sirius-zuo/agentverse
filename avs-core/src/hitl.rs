//! HITL (Human-in-the-Loop) protocol adapter for avs-core.
//!
//! `HitlHook` is the only cross-crate protocol adapter between avs-core
//! and avs-hitl — this is a necessary architectural trade-off:
//! `RunStrategy` must accept the hook without importing avs-hitl.

use serde_json::Value;
use uuid::Uuid;

pub type ApprovalId = Uuid;

#[async_trait::async_trait]
pub trait HitlHook: Send + Sync {
    /// Returns Some((approval_id, kind_json)) if the call is intercepted.
    /// Returns None if the tool is allowed to proceed.
    async fn check_tool(&self, tool_name: &str, args: &Value) -> Option<(ApprovalId, String)>;
}

pub struct HitlInterrupt {
    pub approval_id: ApprovalId,
    pub kind_json: String,
    pub history: Vec<crate::memory::Message>,
    pub pending_calls: Vec<crate::tool::ToolCall>,
    pub active_tool_names: Vec<String>,
}

/// The string wire format used to smuggle a HITL interrupt out of a strategy
/// through an `AgentError::Memory` message:
/// `HITL:{uuid}:{kind_b64}:{history_b64}:{calls_b64}`
///
/// This is the ONLY implementation of this format — do not hand-roll
/// encoding or decoding elsewhere. `active_tool_names` travels out-of-band.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitlWire {
    pub approval_id: ApprovalId,
    pub kind_json: String,
    pub history_json: String,
    pub pending_calls_json: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HitlWireError {
    #[error("missing 'HITL:' prefix")]
    BadPrefix,
    #[error("expected 5 wire segments, found {0}")]
    MissingSegments(usize),
    #[error("invalid approval id: {0}")]
    BadApprovalId(String),
    #[error("invalid base64 in {segment} segment")]
    BadBase64 { segment: &'static str },
    #[error("invalid UTF-8 in {segment} segment")]
    BadUtf8 { segment: &'static str },
}

impl HitlWire {
    pub const PREFIX: &'static str = "HITL:";

    pub fn is_wire(msg: &str) -> bool {
        msg.starts_with(Self::PREFIX)
    }

    pub fn encode(&self) -> String {
        use base64::{engine::general_purpose::STANDARD, Engine};
        format!(
            "HITL:{}:{}:{}:{}",
            self.approval_id,
            STANDARD.encode(self.kind_json.as_bytes()),
            STANDARD.encode(self.history_json.as_bytes()),
            STANDARD.encode(self.pending_calls_json.as_bytes()),
        )
    }

    pub fn parse(msg: &str) -> Result<Self, HitlWireError> {
        if !Self::is_wire(msg) {
            return Err(HitlWireError::BadPrefix);
        }
        let parts: Vec<&str> = msg.splitn(5, ':').collect();
        if parts.len() < 5 {
            return Err(HitlWireError::MissingSegments(parts.len()));
        }
        let approval_id: ApprovalId = parts[1]
            .parse()
            .map_err(|_| HitlWireError::BadApprovalId(parts[1].to_string()))?;
        Ok(Self {
            approval_id,
            kind_json: Self::decode_segment(parts[2], "kind")?,
            history_json: Self::decode_segment(parts[3], "history")?,
            pending_calls_json: Self::decode_segment(parts[4], "pending_calls")?,
        })
    }

    fn decode_segment(b64: &str, segment: &'static str) -> Result<String, HitlWireError> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let bytes = STANDARD
            .decode(b64)
            .map_err(|_| HitlWireError::BadBase64 { segment })?;
        String::from_utf8(bytes).map_err(|_| HitlWireError::BadUtf8 { segment })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct AlwaysBlockHook;

    #[async_trait::async_trait]
    impl HitlHook for AlwaysBlockHook {
        async fn check_tool(&self, tool_name: &str, _args: &Value) -> Option<(ApprovalId, String)> {
            Some((Uuid::new_v4(), format!("{{\"tool\":\"{}\"}}", tool_name)))
        }
    }

    #[tokio::test]
    async fn hook_returns_approval_id_for_any_tool() {
        let hook: Arc<dyn HitlHook> = Arc::new(AlwaysBlockHook);
        let result = hook.check_tool("exec_command", &Value::Null).await;
        assert!(result.is_some());
    }

    struct NeverBlockHook;

    #[async_trait::async_trait]
    impl HitlHook for NeverBlockHook {
        async fn check_tool(&self, _: &str, _: &Value) -> Option<(ApprovalId, String)> {
            None
        }
    }

    #[tokio::test]
    async fn hook_returns_none_for_safe_tool() {
        let hook: Arc<dyn HitlHook> = Arc::new(NeverBlockHook);
        let result = hook.check_tool("file_read", &Value::Null).await;
        assert!(result.is_none());
    }

    fn sample_wire() -> HitlWire {
        HitlWire {
            approval_id: Uuid::new_v4(),
            kind_json: r#"{"ToolApproval":{"tool":"exec_command"}}"#.to_string(),
            history_json: r#"[{"role":"user","content":"hi: colon"}]"#.to_string(),
            pending_calls_json: r#"[{"name":"exec_command","args":{"cmd":"ls"}}]"#.to_string(),
        }
    }

    #[test]
    fn wire_round_trips() {
        let wire = sample_wire();
        let parsed = HitlWire::parse(&wire.encode()).unwrap();
        assert_eq!(parsed, wire);
    }

    #[test]
    fn wire_round_trips_multibyte_utf8() {
        let mut wire = sample_wire();
        wire.kind_json = "résumé: こんにちは".to_string();
        let parsed = HitlWire::parse(&wire.encode()).unwrap();
        assert_eq!(parsed, wire);
    }

    #[test]
    fn encoded_segments_contain_no_colons_beyond_separators() {
        let wire = sample_wire();
        // 5 segments exactly: HITL, uuid, kind, history, calls
        assert_eq!(wire.encode().split(':').count(), 5);
    }

    #[test]
    fn parse_rejects_missing_prefix() {
        assert_eq!(HitlWire::parse("NOPE:abc"), Err(HitlWireError::BadPrefix));
    }

    #[test]
    fn parse_rejects_truncated_message() {
        let err = HitlWire::parse(&format!("HITL:{}:onlyone", Uuid::new_v4())).unwrap_err();
        assert!(matches!(err, HitlWireError::MissingSegments(3)));
    }

    #[test]
    fn parse_rejects_bad_uuid() {
        let err = HitlWire::parse("HITL:not-a-uuid:a:a:a").unwrap_err();
        assert!(matches!(err, HitlWireError::BadApprovalId(_)));
    }

    #[test]
    fn parse_rejects_bad_base64() {
        let err =
            HitlWire::parse(&format!("HITL:{}:!!!notb64!!!:a:a", Uuid::new_v4())).unwrap_err();
        assert!(matches!(err, HitlWireError::BadBase64 { segment: "kind" }));
    }

    #[test]
    fn is_wire_detects_prefix() {
        assert!(HitlWire::is_wire("HITL:whatever"));
        assert!(!HitlWire::is_wire("plain text"));
    }
}
