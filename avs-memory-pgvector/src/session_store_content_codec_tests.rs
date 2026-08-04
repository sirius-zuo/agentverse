use super::*;
use agentverse::memory::ContentBlock;

#[test]
fn decode_content_falls_back_to_text_for_legacy_plain_string() {
    // Rows written before this refactor hold a bare string like "hello",
    // not JSON. decode_content must treat that as a single Text block
    // instead of failing.
    let decoded = decode_content("hello");
    assert_eq!(
        decoded,
        vec![ContentBlock::Text {
            text: "hello".to_string()
        }]
    );
}

#[test]
fn encode_then_decode_roundtrips_text_content() {
    let content = vec![ContentBlock::Text {
        text: "roundtrip me".to_string(),
    }];
    let encoded = encode_content(&content);
    let decoded = decode_content(&encoded);
    assert_eq!(decoded, content);
}

#[test]
fn encode_then_decode_roundtrips_tool_use_and_result() {
    let content = vec![
        ContentBlock::Text {
            text: "let me check".to_string(),
        },
        ContentBlock::ToolUse {
            id: "call_1".to_string(),
            name: "file_read".to_string(),
            input: serde_json::json!({"path": "foo.txt"}),
        },
        ContentBlock::ToolResult {
            tool_use_id: "call_1".to_string(),
            content: "file contents".to_string(),
            is_error: false,
        },
    ];
    let encoded = encode_content(&content);
    let decoded = decode_content(&encoded);
    assert_eq!(decoded, content);
}
