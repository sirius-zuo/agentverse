use axum::{response::IntoResponse, Json};

pub async fn openapi_json() -> impl IntoResponse {
    // Build the GET /sessions/{session_id}/messages response schema in
    // parts to avoid hitting serde_json::json! recursion limits.
    let text_block = serde_json::json!({
        "type": "object",
        "required": ["type", "text"],
        "properties": { "type": { "const": "text" }, "text": { "type": "string" } }
    });
    let tool_use_block = serde_json::json!({
        "type": "object",
        "required": ["type", "id", "name", "input"],
        "properties": { "type": { "const": "tool_use" }, "id": { "type": "string" }, "name": { "type": "string" }, "input": {} }
    });
    let tool_result_block = serde_json::json!({
        "type": "object",
        "required": ["type", "tool_use_id", "content", "is_error"],
        "properties": { "type": { "const": "tool_result" }, "tool_use_id": { "type": "string" }, "content": { "type": "string" }, "is_error": { "type": "boolean" } }
    });
    let content_schema = serde_json::json!({
        "type": "array",
        "description": "Tagged content blocks. Breaking change from the pre-native-tool-calling flat-string shape (see docs/superpowers/specs/2026-07-28-native-tool-calling-design.md) — this is an array of typed blocks, not a plain string.",
        "items": { "oneOf": [text_block, tool_use_block, tool_result_block] }
    });
    let message_item = serde_json::json!({
        "type": "object",
        "required": ["sequence_num", "role", "content"],
        "properties": {
            "sequence_num": { "type": "integer" },
            "role": { "type": "string", "enum": ["system", "user", "assistant", "tool"] },
            "content": content_schema
        }
    });
    let messages_property = serde_json::json!({
        "type": "array",
        "items": message_item
    });
    let messages_response_schema = serde_json::json!({
        "type": "object",
        "properties": { "messages": messages_property }
    });
    let get_messages_response = serde_json::json!({
        "summary": "List messages (paginated)",
        "parameters": [
            { "name": "user_id", "in": "query", "required": true, "schema": { "type": "string" } },
            { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 50 } },
            { "name": "before", "in": "query", "schema": { "type": "integer" }, "description": "Return messages with sequence_num < before" }
        ],
        "responses": {
            "200": {
                "description": "messages array",
                "content": { "application/json": { "schema": messages_response_schema }}
            }
        }
    });

    Json(serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "AgentVerse HTTP API",
            "version": "1.0.0",
            "description": "Agent invocation and session management endpoints."
        },
        "paths": {
            "/v1/health": {
                "get": { "summary": "Health check", "responses": { "200": { "description": "healthy" } } }
            },
            "/v1/ready": {
                "get": { "summary": "Readiness probe", "responses": { "200": { "description": "ready" } } }
            },
            "/v1/invoke": {
                "post": {
                    "summary": "Stateless invocation",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "required": ["user_id", "message"],
                            "properties": {
                                "user_id": { "type": "string" },
                                "message": { "type": "string" }
                            }
                        }}}
                    },
                    "responses": {
                        "200": { "description": "Agent reply" },
                        "400": { "description": "Empty message" },
                        "429": { "description": "Rate limited" }
                    }
                }
            },
            "/v1/sessions": {
                "post": { "summary": "Create session", "responses": { "201": { "description": "session_id" } } }
            },
            "/v1/sessions/{session_id}/messages": {
                "post": { "summary": "Send message", "responses": { "200": { "description": "reply" } } },
                "get": get_messages_response
            }
        }
    }))
}
