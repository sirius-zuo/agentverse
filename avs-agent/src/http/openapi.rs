use axum::{response::IntoResponse, Json};

pub async fn openapi_json() -> impl IntoResponse {
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
                "get": {
                    "summary": "List messages (paginated)",
                    "parameters": [
                        { "name": "user_id", "in": "query", "required": true, "schema": { "type": "string" } },
                        { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 50 } },
                        { "name": "before", "in": "query", "schema": { "type": "integer" }, "description": "Return messages with sequence_num < before" }
                    ],
                    "responses": { "200": { "description": "messages array" } }
                }
            }
        }
    }))
}
