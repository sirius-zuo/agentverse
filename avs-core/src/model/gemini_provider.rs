use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModelProvider;
use crate::error::ModelError;
use crate::model::{GenerateRequest, GenerateResponse, UsageStats};

#[derive(Debug, Clone, Default)]
pub struct GeminiProvider;

#[derive(Debug, Clone, Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Clone, Serialize)]
struct GeminiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiToolConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum GeminiPart {
    Text { text: String },
}

#[derive(Debug, Clone, Serialize)]
struct GeminiToolConfig {
    functions: Vec<GeminiFunction>,
}

#[derive(Debug, Clone, Serialize)]
struct GeminiFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self
    }
}

impl ModelProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn build_request(
        &self,
        _model: &str,
        request: GenerateRequest,
    ) -> Result<serde_json::Value, ModelError> {
        use crate::memory::{ContentBlock, MessageRole};

        if request.tools.is_some() {
            return Err(ModelError::InvalidResponse(
                "GeminiProvider does not support native tool-calling (request.tools) — \
                 Gemini is out of scope for this project's strict tool-calling support"
                    .to_string(),
            ));
        }

        for m in &request.messages {
            if m.content
                .iter()
                .any(|b| !matches!(b, ContentBlock::Text { .. }))
            {
                return Err(ModelError::InvalidResponse(
                    "GeminiProvider does not support native tool-calling content blocks \
                     (ToolUse/ToolResult) — only Text blocks are supported"
                        .to_string(),
                ));
            }
        }

        let system_instruction = request.system.map(|s| GeminiSystemInstruction {
            parts: vec![GeminiPart::Text { text: s }],
        });

        let contents: Vec<GeminiContent> = request
            .messages
            .into_iter()
            .filter_map(|m| {
                let text = m.as_text();
                let role = match m.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "model",
                    MessageRole::Tool => "user",
                    MessageRole::System => return None,
                };
                Some(GeminiContent {
                    role: role.to_string(),
                    parts: vec![GeminiPart::Text { text }],
                })
            })
            .collect();

        let gemini_tools = request.tools.map(|t| {
            vec![GeminiToolConfig {
                functions: t
                    .into_iter()
                    .map(|tool| GeminiFunction {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    })
                    .collect(),
            }]
        });

        let req = GeminiRequest {
            system_instruction,
            contents,
            tools: gemini_tools,
        };

        serde_json::to_value(req).map_err(|e| ModelError::InvalidResponse(e.to_string()))
    }

    fn parse_response(&self, body: &str) -> Result<GenerateResponse, ModelError> {
        let resp: GeminiResponse =
            serde_json::from_str(body).map_err(|e| ModelError::InvalidResponse(e.to_string()))?;

        let content = resp
            .candidates
            .into_iter()
            .next()
            .and_then(|c| {
                c.content.parts.into_iter().next().map(|p| match p {
                    GeminiPart::Text { text } => text,
                })
            })
            .ok_or_else(|| ModelError::InvalidResponse("No content in response".to_string()))?;

        Ok(GenerateResponse {
            content: vec![crate::memory::ContentBlock::Text { text: content }],
            usage: UsageStats::default(), // Gemini context caching is a separate API
        })
    }

    fn request_headers(&self, _api_key: &str) -> HeaderMap {
        // Gemini auth uses ?key=... query param, not headers
        HeaderMap::new()
    }

    fn endpoint_path(&self, model: &str) -> String {
        format!("/v1beta/models/{}:generateContent", model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{ContentBlock, Message, MessageRole};

    #[test]
    fn build_request_rejects_tool_use_block() {
        let provider = GeminiProvider::new();
        let request = GenerateRequest {
            messages: vec![Message {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "calculate".to_string(),
                    input: serde_json::json!({}),
                }],
            }],
            ..Default::default()
        };

        let err = provider.build_request("gemini-pro", request).unwrap_err();
        assert!(matches!(err, ModelError::InvalidResponse(_)));
    }

    #[test]
    fn build_request_rejects_tool_result_block() {
        let provider = GeminiProvider::new();
        let request = GenerateRequest {
            messages: vec![Message {
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: "42".to_string(),
                    is_error: false,
                }],
            }],
            ..Default::default()
        };

        let err = provider.build_request("gemini-pro", request).unwrap_err();
        assert!(matches!(err, ModelError::InvalidResponse(_)));
    }

    #[test]
    fn build_request_rejects_tools_even_with_text_only_messages() {
        let provider = GeminiProvider::new();
        let request = GenerateRequest {
            messages: vec![Message::text(MessageRole::User, "hi")],
            tools: Some(vec![crate::model::ToolDefinition {
                name: "search".to_string(),
                description: "search the web".to_string(),
                parameters: serde_json::json!({}),
            }]),
            ..Default::default()
        };

        let err = provider.build_request("gemini-pro", request).unwrap_err();
        assert!(matches!(err, ModelError::InvalidResponse(_)));
    }

    #[test]
    fn build_request_accepts_text_only_messages() {
        let provider = GeminiProvider::new();
        let request = GenerateRequest {
            system: Some("Be helpful.".to_string()),
            messages: vec![Message::text(MessageRole::User, "hi")],
            ..Default::default()
        };

        let body = provider.build_request("gemini-pro", request).unwrap();
        assert_eq!(body["contents"][0]["parts"][0]["text"], "hi");
        assert_eq!(
            body["system_instruction"]["parts"][0]["text"],
            "Be helpful."
        );
    }
}
