use serde_json::{Map, Value};
use std::fmt;

/// Errors returned when a tool's `schemars`-generated JSON Schema cannot be
/// made strict-mode compatible without silently changing its meaning.
#[derive(Debug)]
pub enum StrictSchemaError {
    /// An open dictionary (arbitrary-key object, e.g. a former
    /// `HashMap<String, String>` field) cannot be made strict: forcing
    /// `additionalProperties: false` onto it would collapse it to one that
    /// can only ever produce `{}`. Redesign the tool's argument as an array
    /// of `{key, value}` pairs instead (see `http_client`'s `HeaderPair`).
    OpenDictionary { path: String },
    /// An optional (not-`required`) field shaped as a bare `$ref` or an
    /// `allOf`-wrapped `$ref`, with no accompanying null signal. `schemars`
    /// produces this identical shape both for a genuine `Option<CustomType>`
    /// missing its null branch and for a `#[serde(default)] CustomType` that
    /// must NOT accept null (`null` would crash that field's own
    /// `serde_json::from_value` deserialization) — the two are
    /// indistinguishable from the schema alone, so this is rejected rather
    /// than guessed. Fix: use genuine `Option<T>` (no `#[serde(default)]`
    /// override) when the field should be nullable.
    AmbiguousOptionalRef { path: String },
}

impl fmt::Display for StrictSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrictSchemaError::OpenDictionary { path } => write!(
                f,
                "tool schema at '{path}' is an open dictionary (arbitrary-key object), \
                 which is incompatible with strict mode — use an array of \
                 {{key, value}} pairs instead"
            ),
            StrictSchemaError::AmbiguousOptionalRef { path } => write!(
                f,
                "tool schema at '{path}' is an optional $ref-typed field with no null \
                 signal — schemars produces this identical shape for both a genuine \
                 nullable Option<T> and a non-nullable #[serde(default)] field, so it \
                 can't be made strict without guessing. Use Option<T> without a \
                 #[serde(default)] override for genuine nullability."
            ),
        }
    }
}

impl std::error::Error for StrictSchemaError {}

/// Converts a `schemars`-generated JSON Schema into the strict dialect both
/// Anthropic and OpenAI-compatible tool-calling require: every object node
/// gets `additionalProperties: false` and lists every property as
/// `required`, including object nodes with no properties at all (an
/// empty-args tool) and nodes reached only via `$ref`/`definitions` or a
/// `oneOf`/`anyOf`/`allOf` combinator (e.g. a data-carrying enum's
/// variants).
///
/// For a property not in the original `required` list: if it's already
/// nullable (schemars' own `type` array/string containing `"null"`, or an
/// `anyOf` branch with `{"type":"null"}`), it's left untouched. If it has no
/// `"type"` key and no `$ref`/`allOf` at all (e.g. an untyped
/// `Option<serde_json::Value>`), it's made explicitly nullable — this is
/// unambiguous, since every such shape observed corresponds to a genuinely
/// nullable field. If it has no `"type"` key but *is* a bare `$ref` or
/// `allOf`-wrapped `$ref` with no null signal, that's ambiguous (see
/// `StrictSchemaError::AmbiguousOptionalRef`) and rejected rather than
/// guessed.
pub fn to_strict_schema(mut schema: Value) -> Result<Value, StrictSchemaError> {
    if let Some(defs) = schema
        .get_mut("definitions")
        .and_then(|d| d.as_object_mut())
    {
        let keys: Vec<String> = defs.keys().cloned().collect();
        for key in keys {
            let def = defs.get_mut(&key).expect("key just read from this map");
            strictify_node(def, &format!("definitions.{key}"))?;
        }
    }
    strictify_node(&mut schema, "$")?;
    Ok(schema)
}

fn strictify_node(node: &mut Value, path: &str) -> Result<(), StrictSchemaError> {
    let Some(obj) = node.as_object_mut() else {
        return Ok(());
    };

    match obj.get("additionalProperties") {
        Some(Value::Object(_)) | Some(Value::Bool(true)) => {
            return Err(StrictSchemaError::OpenDictionary {
                path: path.to_string(),
            });
        }
        _ => {}
    }

    for combinator in ["oneOf", "anyOf", "allOf"] {
        if let Some(Value::Array(variants)) = obj.get_mut(combinator) {
            for (i, variant) in variants.iter_mut().enumerate() {
                strictify_node(variant, &format!("{path}.{combinator}[{i}]"))?;
            }
        }
    }

    if let Some(items) = obj.get_mut("items") {
        strictify_node(items, &format!("{path}.items"))?;
    }

    let Some(Value::Object(properties_map)) = obj.get("properties").cloned() else {
        // Object-typed node with no "properties" at all (e.g. an
        // empty-args tool like `datetime`) still needs the strict
        // envelope — otherwise it ships with no additionalProperties
        // declared at all.
        if obj.get("type").and_then(Value::as_str) == Some("object") {
            obj.insert("properties".to_string(), Value::Object(Map::new()));
            obj.insert("required".to_string(), Value::Array(vec![]));
            obj.insert("additionalProperties".to_string(), Value::Bool(false));
        }
        return Ok(());
    };

    let existing_required: Vec<String> = obj
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut new_properties = Map::new();
    for (key, mut prop_schema) in properties_map {
        strictify_node(&mut prop_schema, &format!("{path}.{key}"))?;
        if !existing_required.contains(&key) {
            ensure_nullable_or_reject(&mut prop_schema, &format!("{path}.{key}"))?;
        }
        new_properties.insert(key, prop_schema);
    }

    let all_required: Vec<Value> = new_properties
        .keys()
        .map(|k| Value::String(k.clone()))
        .collect();

    obj.insert("properties".to_string(), Value::Object(new_properties));
    obj.insert("required".to_string(), Value::Array(all_required));
    obj.insert("additionalProperties".to_string(), Value::Bool(false));
    Ok(())
}

/// See `to_strict_schema`'s doc comment for the full decision rationale.
fn ensure_nullable_or_reject(prop_schema: &mut Value, path: &str) -> Result<(), StrictSchemaError> {
    let Some(obj) = prop_schema.as_object_mut() else {
        return Ok(());
    };

    if obj.contains_key("type") {
        // Already has a concrete type: either schemars already added
        // "null" to it (simple Option<T>), or it's a non-nullable
        // #[serde(default)] field (e.g. Vec<T>) that must stay that way.
        // Either way, leave it exactly as schemars produced it.
        return Ok(());
    }

    if let Some(Value::Array(variants)) = obj.get("anyOf") {
        let already_nullable = variants
            .iter()
            .any(|v| v.get("type") == Some(&Value::String("null".to_string())));
        if already_nullable {
            return Ok(());
        }
    }

    if obj.contains_key("$ref") || obj.contains_key("allOf") {
        return Err(StrictSchemaError::AmbiguousOptionalRef {
            path: path.to_string(),
        });
    }

    // No type, no $ref/allOf, no null-bearing anyOf: an untyped schema
    // like Option<serde_json::Value> that schemars couldn't give a
    // concrete type to. Unambiguously nullable — make that explicit.
    obj.insert(
        "type".to_string(),
        serde_json::json!(["string", "number", "object", "array", "boolean", "null"]),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calculator::Calculator;
    use crate::http_client::HttpClient;
    use agentverse::ErasedTool;

    #[test]
    fn calculator_schema_becomes_strict_with_no_optional_fields() {
        let schema = Calculator.schema();
        let input_schema = schema["input_schema"].clone();
        let strict = to_strict_schema(input_schema).unwrap();

        assert_eq!(strict["additionalProperties"], Value::Bool(false));
        let required: Vec<&str> = strict["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(required.contains(&"operation"));
        assert!(required.contains(&"a"));
        assert!(required.contains(&"b"));
        assert_eq!(required.len(), 3);
    }

    #[test]
    fn http_client_schema_distinguishes_option_from_default_fields() {
        let schema = HttpClient.schema();
        let input_schema = schema["input_schema"].clone();
        let strict = to_strict_schema(input_schema).unwrap();

        let required: Vec<&str> = strict["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        // Both previously-optional fields are now required...
        assert!(required.contains(&"headers"));
        assert!(required.contains(&"body"));

        // ...but only `body` (a genuine Option<Value>) is nullable.
        let body_type = strict["properties"]["body"]["type"].as_array().unwrap();
        assert!(body_type.iter().any(|v| v == "null"));

        // `headers` (Vec<HeaderPair> with #[serde(default)]) keeps its plain
        // array type — no "null" added, since sending null would crash the
        // tool's own deserialization (verified separately in
        // Step 3's design rationale).
        assert_eq!(strict["properties"]["headers"]["type"], "array");

        // additionalProperties:false must also reach HeaderPair inside definitions.
        let header_pair_def = &strict["definitions"]["HeaderPair"];
        assert_eq!(header_pair_def["additionalProperties"], Value::Bool(false));
        let pair_required: std::collections::HashSet<&str> = header_pair_def["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            pair_required,
            std::collections::HashSet::from(["key", "value"])
        );
    }

    #[test]
    fn open_dictionary_shape_is_rejected() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "object",
                    "additionalProperties": {"type": "string"}
                }
            },
            "required": ["tags"]
        });
        let err = to_strict_schema(schema).unwrap_err();
        assert!(matches!(err, StrictSchemaError::OpenDictionary { .. }));
    }

    #[test]
    fn anyof_null_already_present_is_left_unchanged() {
        // Option<SomeStruct> produces {"anyOf": [{"$ref": "..."}, {"type": "null"}]}.
        // The existing anyOf should be left completely untouched, not have a
        // redundant type array added alongside.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "opt_struct": {
                    "anyOf": [
                        {"$ref": "#/definitions/Something"},
                        {"type": "null"}
                    ]
                }
            },
            "required": []
        });
        let strict = to_strict_schema(schema).unwrap();

        assert!(strict["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "opt_struct"));

        let opt_struct = &strict["properties"]["opt_struct"];
        assert!(opt_struct["anyOf"].is_array());
        assert_eq!(opt_struct["anyOf"].as_array().unwrap().len(), 2);
        assert!(opt_struct["anyOf"][0]["$ref"].is_string());
        assert_eq!(opt_struct["anyOf"][1]["type"], "null");
        assert!(opt_struct.get("type").is_none() || opt_struct["type"].is_null());
    }

    #[test]
    fn serde_default_option_t_gets_nullability_added() {
        // #[serde(default)] Option<T> produces {"default": null} with no
        // "type" key. This must become nullable (either type array with
        // null or anyOf with null).
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "opt_field": { "default": null }
            },
            "required": []
        });
        let strict = to_strict_schema(schema).unwrap();

        assert!(strict["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "opt_field"));

        let opt_field = &strict["properties"]["opt_field"];
        let has_nullability = if let Some(arr) = opt_field["type"].as_array() {
            arr.iter().any(|v| v == "null")
        } else {
            opt_field["anyOf"].is_array()
        };
        assert!(
            has_nullability,
            "opt_field should be nullable after transformation"
        );
    }

    #[test]
    fn bare_ref_without_null_signal_is_rejected_as_ambiguous() {
        // A bare `{"$ref": ...}` not in `required`, with no accompanying null
        // signal, is genuinely ambiguous: schemars produces this identical
        // shape for a #[serde(default)] CustomType (must NOT accept null) and
        // for an Option<CustomType> missing its null branch. Must be
        // rejected, not guessed at.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "flexible": { "$ref": "#/definitions/Something" }
            },
            "required": []
        });
        let err = to_strict_schema(schema).unwrap_err();
        assert!(matches!(
            err,
            StrictSchemaError::AmbiguousOptionalRef { .. }
        ));
    }

    #[test]
    fn allof_wrapped_ref_without_null_signal_is_rejected_as_ambiguous() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "flexible": {
                    "allOf": [{ "$ref": "#/definitions/Something" }],
                    "description": "some doc-commented ref-typed default field"
                }
            },
            "required": []
        });
        let err = to_strict_schema(schema).unwrap_err();
        assert!(matches!(
            err,
            StrictSchemaError::AmbiguousOptionalRef { .. }
        ));
    }

    #[test]
    fn empty_args_object_gets_strict_envelope() {
        // Mirrors DateTimeArgs {} — schemars emits {"type":"object"} with no
        // properties key at all for a struct with no fields.
        let schema = serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "EmptyArgs",
            "type": "object"
        });
        let strict = to_strict_schema(schema).unwrap();
        assert_eq!(strict["additionalProperties"], Value::Bool(false));
        assert_eq!(strict["properties"], serde_json::json!({}));
        assert_eq!(strict["required"], serde_json::json!([]));
    }

    #[test]
    fn additional_properties_true_is_rejected_as_open_dictionary() {
        let schema = serde_json::json!({
            "type": "object",
            "additionalProperties": true
        });
        let err = to_strict_schema(schema).unwrap_err();
        assert!(matches!(err, StrictSchemaError::OpenDictionary { .. }));
    }

    #[test]
    fn oneof_variants_are_recursively_strictified() {
        // Mirrors a data-carrying enum's schemars representation: each
        // variant is its own object schema nested inside "oneOf".
        let schema = serde_json::json!({
            "definitions": {
                "Action": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "Send": {
                                    "type": "object",
                                    "properties": {
                                        "to": {"type": "string"},
                                        "body": {"type": "string"}
                                    },
                                    "required": ["to", "body"]
                                }
                            },
                            "required": ["Send"]
                        }
                    ]
                }
            },
            "type": "object",
            "properties": {
                "action": { "$ref": "#/definitions/Action" }
            },
            "required": ["action"]
        });
        let strict = to_strict_schema(schema).unwrap();
        let variant = &strict["definitions"]["Action"]["oneOf"][0];
        assert_eq!(variant["additionalProperties"], Value::Bool(false));
        let inner = &variant["properties"]["Send"];
        assert_eq!(inner["additionalProperties"], Value::Bool(false));
        let inner_required: std::collections::HashSet<&str> = inner["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            inner_required,
            std::collections::HashSet::from(["to", "body"])
        );
    }

    #[test]
    fn every_registered_tool_schema_becomes_fully_strict() {
        // Structural invariant test across every real tool in this crate,
        // not just the two hand-picked fixtures above: every object-typed
        // node in the strictified schema must have additionalProperties:
        // false and a required list exactly matching its properties' keys.
        // This is the test that would have caught the empty-args-tool bug
        // (datetime) — it exercises shapes the module author didn't
        // specifically write a test for.
        use crate::datetime::DateTimeTool;
        use crate::file_search::FileSearch;
        use crate::registry::ToolRegistry;
        use crate::shell::ShellTool;
        use crate::web_search::WebSearch;
        use std::time::Duration;

        let registry = ToolRegistry::new();
        registry.register(Calculator);
        registry.register(DateTimeTool);
        registry.register(FileSearch);
        registry.register(HttpClient);
        registry.register(ShellTool::new(".", Duration::from_secs(5), vec![]));
        registry.register(WebSearch);

        for schema in registry.schema() {
            let name = schema["name"].as_str().unwrap().to_string();
            let input_schema = schema["input_schema"].clone();
            let strict = to_strict_schema(input_schema)
                .unwrap_or_else(|e| panic!("tool '{name}' schema failed to strictify: {e}"));
            assert_all_object_nodes_are_strict(&strict, &name);
        }
    }

    fn assert_all_object_nodes_are_strict(node: &Value, tool_name: &str) {
        match node {
            Value::Object(obj) => {
                if obj.get("type").and_then(Value::as_str) == Some("object") {
                    assert_eq!(
                        obj.get("additionalProperties"),
                        Some(&Value::Bool(false)),
                        "tool '{tool_name}': object node missing additionalProperties:false: {obj:?}"
                    );
                    let props: std::collections::HashSet<&str> = obj
                        .get("properties")
                        .and_then(Value::as_object)
                        .map(|m| m.keys().map(String::as_str).collect())
                        .unwrap_or_default();
                    let required: std::collections::HashSet<&str> = obj
                        .get("required")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                    assert_eq!(
                        props, required,
                        "tool '{tool_name}': required does not match properties keys"
                    );
                }
                for v in obj.values() {
                    assert_all_object_nodes_are_strict(v, tool_name);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    assert_all_object_nodes_are_strict(v, tool_name);
                }
            }
            _ => {}
        }
    }
}
