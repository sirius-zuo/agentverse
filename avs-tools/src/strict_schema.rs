use serde_json::{Map, Value};
use std::fmt;

/// A tool schema shaped like an open dictionary (arbitrary string keys
/// mapped to a value schema, e.g. a former `HashMap<String, String>`
/// field) cannot be made strict-mode compatible: `additionalProperties:
/// false` on such a node would force it to always be `{}`. The fix is to
/// redesign the tool's argument as an array of `{key, value}` pairs
/// instead (see `http_client`'s `HeaderPair` for the pattern).
#[derive(Debug)]
pub struct StrictSchemaError {
    pub path: String,
}

impl fmt::Display for StrictSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tool schema at '{}' is an open dictionary (arbitrary-key object), \
             which is incompatible with strict mode — use an array of \
             {{key, value}} pairs instead",
            self.path
        )
    }
}

impl std::error::Error for StrictSchemaError {}

/// Converts a `schemars`-generated JSON Schema into the strict dialect
/// both Anthropic and OpenAI-compatible tool-calling require: every
/// object node gets `additionalProperties: false` and lists every
/// property as `required`.
///
/// Genuine `Option<T>` fields are already nullable in schemars' own
/// output (`"type": [.., "null"]`) and are left as-is beyond being added
/// to `required`. Fields using `#[serde(default)]` on a non-`Option`
/// type are marked by schemars with a `"type"` key instead of being
/// absent — these are added to `required` WITHOUT adding "null" to
/// their type, because a strict-mode model sending an explicit `null`
/// there would crash the tool's own `serde_json::from_value`
/// deserialization (`#[serde(default)]` only supplies its fallback when
/// the JSON key is absent, not when it's present as `null`).
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

    if matches!(obj.get("additionalProperties"), Some(Value::Object(_))) {
        return Err(StrictSchemaError {
            path: path.to_string(),
        });
    }

    let Some(Value::Object(properties_map)) = obj.get("properties").cloned() else {
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
        if let Some(items) = prop_schema.get_mut("items") {
            strictify_node(items, &format!("{path}.{key}.items"))?;
        }
        if !existing_required.contains(&key) && has_no_type(&prop_schema) {
            make_nullable_if_true_optional(&mut prop_schema);
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

/// Checks if a property schema has no "type" field (indicating a true
/// Option<T> rather than a field with a default value).
fn has_no_type(prop_schema: &Value) -> bool {
    match prop_schema {
        Value::Object(obj) => !obj.contains_key("type"),
        _ => true,
    }
}

/// Adds "null" to a property's type only when the property is a genuine
/// `Option<T>` — distinguished from a `#[serde(default)]` non-Option
/// field by the absence of a "type" key. Never adds "null" when a
/// "type" key is present.
fn make_nullable_if_true_optional(prop_schema: &mut Value) {
    let Some(obj) = prop_schema.as_object_mut() else {
        return;
    };
    if obj.contains_key("default") {
        return;
    }
    match obj.get("type").cloned() {
        Some(Value::String(t)) if t != "null" => {
            obj.insert("type".to_string(), serde_json::json!([t, "null"]));
        }
        Some(Value::Array(mut arr)) if !arr.iter().any(|v| v == "null") => {
            arr.push(Value::String("null".to_string()));
            obj.insert("type".to_string(), Value::Array(arr));
        }
        None => {
            // If there's no type, check if this is a $ref
            if obj.contains_key("$ref") {
                // For $ref without type, wrap in anyOf to add null
                let original = Value::Object(obj.clone());
                obj.clear();
                obj.insert(
                    "anyOf".to_string(),
                    serde_json::json!([original, {"type": "null"}]),
                );
            } else {
                // For other schemas without type, create a type that allows any JSON value plus null
                obj.insert(
                    "type".to_string(),
                    serde_json::json!(["string", "number", "object", "array", "boolean", "null"]),
                );
            }
        }
        _ => {}
    }
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
        assert!(err.path.contains("tags"), "path was: {}", err.path);
    }

    #[test]
    fn optional_field_without_type_or_default_gets_anyof_null_wrapped() {
        // Synthetic: no current tool produces this shape (every real
        // optional field is either a true Option<T>, already nullable
        // via schemars, or a #[serde(default)] field with a "default"
        // key) — this pins the defensive fallback branch so it's a
        // deliberate, tested behavior rather than untested dead code.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "flexible": { "$ref": "#/definitions/Something" }
            },
            "required": []
        });
        let strict = to_strict_schema(schema).unwrap();
        assert!(strict["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "flexible"));
        assert!(strict["properties"]["flexible"]["anyOf"].is_array());
    }
}
