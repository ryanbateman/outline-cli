use serde_json::Value;
use std::fmt;

/// Validation error for a JSON payload against an OpenAPI schema.
#[derive(Debug, Clone)]
pub struct SchemaError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// Validate a JSON payload against an OpenAPI request body schema.
///
/// The schema is the raw OpenAPI schema (may contain `allOf`, `$ref`, etc.).
/// The spec is the full OpenAPI spec (needed for `$ref` resolution).
///
/// Returns a list of validation errors (empty = valid).
pub fn validate_payload(payload: &Value, schema: &Value, spec: &Value) -> Vec<SchemaError> {
    let mut errors = Vec::new();
    validate_value(payload, schema, spec, "", &mut errors);
    errors
}

fn validate_value(
    value: &Value,
    schema: &Value,
    spec: &Value,
    path: &str,
    errors: &mut Vec<SchemaError>,
) {
    // Handle $ref
    if let Some(ref_str) = schema.get("$ref").and_then(|v| v.as_str()) {
        if let Some(resolved) = resolve_ref(ref_str, spec) {
            validate_value(value, &resolved, spec, path, errors);
        }
        return;
    }

    // Handle allOf: merge schemas and validate against all
    if let Some(all_of) = schema.get("allOf").and_then(|v| v.as_array()) {
        for sub_schema in all_of {
            validate_value(value, sub_schema, spec, path, errors);
        }
        return;
    }

    // Handle oneOf: value must match at least one
    if let Some(one_of) = schema.get("oneOf").and_then(|v| v.as_array()) {
        let mut any_valid = false;
        for sub_schema in one_of {
            let mut sub_errors = Vec::new();
            validate_value(value, sub_schema, spec, path, &mut sub_errors);
            if sub_errors.is_empty() {
                any_valid = true;
                break;
            }
        }
        if !any_valid {
            errors.push(SchemaError {
                path: path.to_string(),
                message: "Value does not match any of the expected types".to_string(),
            });
        }
        return;
    }

    // Type checking
    if let Some(type_str) = schema.get("type").and_then(|v| v.as_str()) {
        let type_ok = match type_str {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" | "integer" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true, // Unknown type, pass
        };

        // nullable allows null for any type
        let nullable = schema
            .get("nullable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !(type_ok || nullable && value.is_null()) {
            errors.push(SchemaError {
                path: path.to_string(),
                message: format!("Expected type '{type_str}', got {}", value_type_name(value)),
            });
            return; // Don't validate further if type is wrong
        }

        // For objects, validate properties and required fields
        if type_str == "object"
            && let Some(obj) = value.as_object()
        {
            validate_object(obj, schema, spec, path, errors);
        }

        // For arrays, validate items
        if type_str == "array"
            && let Some(arr) = value.as_array()
            && let Some(items_schema) = schema.get("items")
        {
            for (i, item) in arr.iter().enumerate() {
                let item_path = format!("{path}[{i}]");
                validate_value(item, items_schema, spec, &item_path, errors);
            }
        }

        // For strings, validate format and enum
        if type_str == "string"
            && let Some(s) = value.as_str()
        {
            validate_string_format(s, schema, path, errors);
        }

        // Enum validation (works for any type)
        if let Some(enum_values) = schema.get("enum").and_then(|v| v.as_array())
            && !enum_values.contains(value)
        {
            let allowed: Vec<String> = enum_values.iter().map(|v| v.to_string()).collect();
            errors.push(SchemaError {
                path: path.to_string(),
                message: format!(
                    "Value {} not in allowed values: [{}]",
                    value,
                    allowed.join(", ")
                ),
            });
        }
    }
}

fn validate_object(
    obj: &serde_json::Map<String, Value>,
    schema: &Value,
    spec: &Value,
    path: &str,
    errors: &mut Vec<SchemaError>,
) {
    // Check required fields
    if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
        for req in required {
            if let Some(field_name) = req.as_str()
                && !obj.contains_key(field_name)
            {
                errors.push(SchemaError {
                    path: format!("{path}.{field_name}"),
                    message: format!("Required field '{field_name}' is missing"),
                });
            }
        }
    }

    // Validate each property against its schema
    if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
        for (key, val) in obj {
            if let Some(prop_schema) = properties.get(key) {
                let prop_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                validate_value(val, prop_schema, spec, &prop_path, errors);
            }
            // Unknown properties are allowed (OpenAPI default)
        }
    }
}

fn validate_string_format(s: &str, schema: &Value, path: &str, errors: &mut Vec<SchemaError>) {
    if let Some(format) = schema.get("format").and_then(|v| v.as_str()) {
        match format {
            "uuid" => {
                // Basic UUID format check: 8-4-4-4-12 hex chars
                let is_uuid = s.len() == 36
                    && s.chars().enumerate().all(|(i, c)| {
                        if i == 8 || i == 13 || i == 18 || i == 23 {
                            c == '-'
                        } else {
                            c.is_ascii_hexdigit()
                        }
                    });
                if !is_uuid {
                    errors.push(SchemaError {
                        path: path.to_string(),
                        message: format!("Invalid UUID format: '{s}'"),
                    });
                }
            }
            "date-time" => {
                // Basic ISO 8601 check: must contain 'T' and end with 'Z' or timezone
                if !s.contains('T') || s.len() < 20 {
                    errors.push(SchemaError {
                        path: path.to_string(),
                        message: format!("Invalid date-time format: '{s}'"),
                    });
                }
            }
            // Other formats (uri, etc.) — don't validate strictly for now
            _ => {}
        }
    }
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Resolve a $ref path against the spec.
fn resolve_ref(ref_path: &str, spec: &Value) -> Option<Value> {
    let path = ref_path.strip_prefix("#/")?;
    let mut current = spec;
    for segment in path.split('/') {
        current = current.get(segment)?;
    }
    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_spec() -> Value {
        json!({})
    }

    #[test]
    fn valid_simple_object() {
        let schema = json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "publish": {"type": "boolean"}
            }
        });
        let payload = json!({"title": "Hello", "publish": true});
        let errors = validate_payload(&payload, &schema, &empty_spec());
        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn missing_required_field() {
        let schema = json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": {"type": "string"}
            }
        });
        let payload = json!({"text": "no title"});
        let errors = validate_payload(&payload, &schema, &empty_spec());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Required field 'title'"));
    }

    #[test]
    fn wrong_type() {
        let schema = json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"}
            }
        });
        let payload = json!({"title": 42});
        let errors = validate_payload(&payload, &schema, &empty_spec());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Expected type 'string'"));
    }

    #[test]
    fn invalid_uuid_format() {
        let schema = json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "format": "uuid"}
            }
        });
        let payload = json!({"id": "not-a-uuid"});
        let errors = validate_payload(&payload, &schema, &empty_spec());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Invalid UUID"));
    }

    #[test]
    fn valid_uuid_format() {
        let schema = json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "format": "uuid"}
            }
        });
        let payload = json!({"id": "550e8400-e29b-41d4-a716-446655440000"});
        let errors = validate_payload(&payload, &schema, &empty_spec());
        assert!(errors.is_empty());
    }

    #[test]
    fn nullable_field_accepts_null() {
        let schema = json!({
            "type": "object",
            "properties": {
                "collectionId": {"type": "string", "format": "uuid", "nullable": true}
            }
        });
        let payload = json!({"collectionId": null});
        let errors = validate_payload(&payload, &schema, &empty_spec());
        assert!(errors.is_empty());
    }

    #[test]
    fn enum_validation() {
        let schema = json!({
            "type": "object",
            "properties": {
                "direction": {"type": "string", "enum": ["ASC", "DESC"]}
            }
        });
        let payload = json!({"direction": "INVALID"});
        let errors = validate_payload(&payload, &schema, &empty_spec());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("not in allowed values"));
    }

    #[test]
    fn allof_validation() {
        let spec = json!({
            "components": {
                "schemas": {
                    "Pagination": {
                        "type": "object",
                        "properties": {
                            "offset": {"type": "number"},
                            "limit": {"type": "number"}
                        }
                    }
                }
            }
        });
        let schema = json!({
            "allOf": [
                {"$ref": "#/components/schemas/Pagination"},
                {
                    "type": "object",
                    "properties": {
                        "collectionId": {"type": "string", "format": "uuid"}
                    }
                }
            ]
        });
        let payload = json!({
            "limit": 25,
            "collectionId": "550e8400-e29b-41d4-a716-446655440000"
        });
        let errors = validate_payload(&payload, &schema, &spec);
        assert!(errors.is_empty());
    }

    #[test]
    fn allof_detects_error_in_composed_schema() {
        let spec = json!({
            "components": {
                "schemas": {
                    "Pagination": {
                        "type": "object",
                        "properties": {
                            "limit": {"type": "number"}
                        }
                    }
                }
            }
        });
        let schema = json!({
            "allOf": [
                {"$ref": "#/components/schemas/Pagination"},
                {
                    "type": "object",
                    "properties": {
                        "collectionId": {"type": "string", "format": "uuid"}
                    }
                }
            ]
        });
        let payload = json!({
            "limit": "not a number",
            "collectionId": "bad-uuid"
        });
        let errors = validate_payload(&payload, &schema, &spec);
        assert!(
            errors.len() >= 2,
            "Expected at least 2 errors, got: {errors:?}"
        );
    }

    #[test]
    fn oneof_string_boolean_number() {
        let schema = json!({
            "type": "object",
            "properties": {
                "value": {
                    "oneOf": [
                        {"type": "string"},
                        {"type": "boolean"},
                        {"type": "number"}
                    ]
                }
            }
        });

        // String should pass
        let errors = validate_payload(&json!({"value": "hello"}), &schema, &empty_spec());
        assert!(errors.is_empty());

        // Boolean should pass
        let errors = validate_payload(&json!({"value": true}), &schema, &empty_spec());
        assert!(errors.is_empty());

        // Number should pass
        let errors = validate_payload(&json!({"value": 42}), &schema, &empty_spec());
        assert!(errors.is_empty());

        // Array should fail
        let errors = validate_payload(&json!({"value": [1, 2, 3]}), &schema, &empty_spec());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn array_items_validation() {
        let schema = json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id"],
                        "properties": {
                            "id": {"type": "string", "format": "uuid"}
                        }
                    }
                }
            }
        });
        let payload = json!({
            "items": [
                {"id": "550e8400-e29b-41d4-a716-446655440000"},
                {"id": "not-valid"}
            ]
        });
        let errors = validate_payload(&payload, &schema, &empty_spec());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].path.contains("[1]"));
    }

    #[test]
    fn validates_documents_create_against_real_spec() {
        let spec_json = include_str!("../../api/spec3.json");
        let spec: Value = serde_json::from_str(spec_json).unwrap();

        // Extract the request body schema for documents.create
        let schema = &spec["paths"]["/documents.create"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"];

        // Valid payload
        let payload = json!({
            "title": "Test Document",
            "collectionId": "550e8400-e29b-41d4-a716-446655440000",
            "text": "# Hello\n\nWorld",
            "publish": true
        });
        let errors = validate_payload(&payload, schema, &spec);
        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");

        // Invalid UUID
        let payload = json!({
            "title": "Test",
            "collectionId": "not-a-uuid"
        });
        let errors = validate_payload(&payload, schema, &spec);
        assert!(!errors.is_empty(), "Should reject invalid UUID");
    }

    #[test]
    fn validates_documents_list_against_real_spec() {
        let spec_json = include_str!("../../api/spec3.json");
        let spec: Value = serde_json::from_str(spec_json).unwrap();

        let schema = &spec["paths"]["/documents.list"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"];

        // Valid payload with allOf composition (Pagination + Sorting + inline)
        let payload = json!({
            "collectionId": "550e8400-e29b-41d4-a716-446655440000",
            "limit": 25,
            "offset": 0
        });
        let errors = validate_payload(&payload, schema, &spec);
        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }
}
