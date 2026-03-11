use serde_json::Value;
use std::fmt;

/// Input validation error.
#[derive(Debug, Clone)]
pub struct InputError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// Validate a resource ID string.
///
/// Rejects IDs containing `?`, `#`, or `%` — these indicate agent hallucinations
/// where query parameters or pre-encoded strings were embedded in the ID.
pub fn validate_resource_id(id: &str) -> Result<(), InputError> {
    if id.is_empty() {
        return Err(InputError {
            path: String::new(),
            message: "Resource ID cannot be empty".to_string(),
        });
    }
    for ch in ['?', '#', '%'] {
        if id.contains(ch) {
            return Err(InputError {
                path: String::new(),
                message: format!(
                    "Resource ID contains invalid character '{ch}'. \
                     IDs must not contain '?', '#', or '%' \
                     (these often indicate hallucinated query params or double-encoding)"
                ),
            });
        }
    }
    reject_control_chars(id, "resource ID")?;
    Ok(())
}

/// Reject control characters below ASCII 0x20, except `\n`, `\r`, `\t`.
///
/// These characters in API payloads indicate corrupt or adversarial input.
pub fn reject_control_chars(s: &str, context: &str) -> Result<(), InputError> {
    for (i, c) in s.chars().enumerate() {
        let code = c as u32;
        if code < 0x20 && c != '\n' && c != '\r' && c != '\t' {
            return Err(InputError {
                path: String::new(),
                message: format!(
                    "Control character U+{code:04X} at position {i} in {context}. \
                     Characters below U+0020 (except newline, carriage return, tab) are not allowed."
                ),
            });
        }
    }
    Ok(())
}

/// Detect double-encoded strings (strings containing `%XX` patterns).
///
/// The HTTP layer handles percent-encoding. Pre-encoded strings in payloads
/// will be double-encoded, producing incorrect values.
pub fn reject_double_encoding(s: &str, context: &str) -> Result<(), InputError> {
    // Look for %XX patterns where XX are hex digits
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = bytes[i + 1];
            let h2 = bytes[i + 2];
            if h1.is_ascii_hexdigit() && h2.is_ascii_hexdigit() {
                return Err(InputError {
                    path: String::new(),
                    message: format!(
                        "Pre-encoded percent sequence '%{}{}' found in {context}. \
                         Do not percent-encode values — the CLI handles encoding at the HTTP layer.",
                        h1 as char, h2 as char
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Determine if a field is an identifier field based on schema and field name.
///
/// A field is considered an ID if:
/// 1. The OpenAPI schema for the field has `format: uuid`, OR
/// 2. The field name is `id` or ends with `Id` (e.g., `collectionId`, `userId`)
///
/// This follows the gws (Google Workspace CLI) pattern of field-aware validation:
/// ID fields get strict validation (reject ?, #, %, double-encoding);
/// content fields get only control-character checks.
fn is_id_field(field_name: &str, schema: Option<&Value>) -> bool {
    // Check schema first — format: uuid is the strongest signal
    if let Some(schema) = schema
        && schema.get("format").and_then(|f| f.as_str()) == Some("uuid")
    {
        return true;
    }

    // Fall back to field name heuristic
    field_name == "id" || field_name.ends_with("Id")
}

/// Resolve a field's schema from a parent object schema.
///
/// Handles both direct `properties` and `allOf` composition.
fn resolve_field_schema<'a>(
    field_name: &str,
    parent_schema: Option<&'a Value>,
) -> Option<&'a Value> {
    let schema = parent_schema?;

    // Direct properties lookup
    if let Some(field_schema) = schema.get("properties").and_then(|p| p.get(field_name)) {
        return Some(field_schema);
    }

    // allOf composition — search each sub-schema's properties
    if let Some(Value::Array(all_of)) = schema.get("allOf") {
        for sub_schema in all_of {
            if let Some(field_schema) = sub_schema.get("properties").and_then(|p| p.get(field_name))
            {
                return Some(field_schema);
            }
        }
    }

    None
}

/// Validate all string values in a JSON payload with field-aware validation.
///
/// Walks the payload recursively and applies validation based on field role:
/// - **ID fields** (format: uuid or name id/*Id): strict validation —
///   control chars, double-encoding, and resource ID checks (reject ?, #, %)
/// - **Content fields** (everything else): control-character rejection only
///
/// The schema parameter enables field-role detection from the OpenAPI spec.
/// When None, falls back to field-name heuristics.
pub fn validate_json_payload(value: &Value, schema: Option<&Value>) -> Vec<InputError> {
    let mut errors = Vec::new();
    validate_json_recursive(value, "", schema, &mut errors);
    errors
}

fn validate_json_recursive(
    value: &Value,
    path: &str,
    schema: Option<&Value>,
    errors: &mut Vec<InputError>,
) {
    match value {
        Value::String(s) => {
            let field_name = path.rsplit('.').next().unwrap_or(path);
            // Strip array index suffix for field name matching (e.g., "items[0]" → "items")
            let clean_field_name = field_name.split('[').next().unwrap_or(field_name);
            let id_field = is_id_field(clean_field_name, schema);

            // Control character check — applies to ALL fields
            if let Err(mut e) = reject_control_chars(s, &format!("field '{path}'")) {
                e.path = path.to_string();
                errors.push(e);
            }

            if id_field {
                // Strict validation for ID fields: double-encoding + resource ID checks
                if let Err(mut e) = reject_double_encoding(s, &format!("field '{path}'")) {
                    e.path = path.to_string();
                    errors.push(e);
                }

                if let Err(mut e) = validate_resource_id(s) {
                    e.path = path.to_string();
                    errors.push(e);
                }
            }
            // Content fields: no double-encoding or ?#% checks — these characters
            // are legitimate in document text, search queries, descriptions, etc.
        }
        Value::Object(obj) => {
            for (key, val) in obj {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                // Resolve the field's schema from the parent schema
                let field_schema = resolve_field_schema(key, schema);
                validate_json_recursive(val, &child_path, field_schema, errors);
            }
        }
        Value::Array(arr) => {
            // For arrays, the items share the array's `items` schema
            let items_schema = schema.and_then(|s| s.get("items"));
            for (i, item) in arr.iter().enumerate() {
                let child_path = format!("{path}[{i}]");
                validate_json_recursive(item, &child_path, items_schema, errors);
            }
        }
        _ => {} // Numbers, booleans, null are always valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    // --- Resource ID validation ---

    #[test]
    fn valid_uuid_id() {
        assert!(validate_resource_id("550e8400-e29b-41d4-a716-446655440000").is_ok());
    }

    #[test]
    fn rejects_empty_id() {
        assert!(validate_resource_id("").is_err());
    }

    #[test]
    fn rejects_id_with_query_param() {
        let err = validate_resource_id("abc123?fields=name").unwrap_err();
        assert!(err.message.contains("'?'"));
    }

    #[test]
    fn rejects_id_with_fragment() {
        let err = validate_resource_id("abc123#section").unwrap_err();
        assert!(err.message.contains("'#'"));
    }

    #[test]
    fn rejects_id_with_percent_encoding() {
        let err = validate_resource_id("abc%2F123").unwrap_err();
        assert!(err.message.contains("'%'"));
    }

    // --- Control character rejection ---

    #[test]
    fn accepts_normal_text() {
        assert!(reject_control_chars("Hello, world!\nNew line\ttab", "test").is_ok());
    }

    #[test]
    fn rejects_null_byte() {
        let err = reject_control_chars("hello\x00world", "test").unwrap_err();
        assert!(err.message.contains("U+0000"));
    }

    #[test]
    fn rejects_bell_character() {
        let err = reject_control_chars("hello\x07world", "test").unwrap_err();
        assert!(err.message.contains("U+0007"));
    }

    #[test]
    fn allows_newline_carriage_return_tab() {
        assert!(reject_control_chars("line1\nline2\rline3\tcol", "test").is_ok());
    }

    // --- Double-encoding detection ---

    #[test]
    fn accepts_normal_text_no_encoding() {
        assert!(reject_double_encoding("hello world", "test").is_ok());
    }

    #[test]
    fn rejects_percent_encoded_slash() {
        let err = reject_double_encoding("abc%2Fdef", "test").unwrap_err();
        assert!(err.message.contains("%2F"));
    }

    #[test]
    fn rejects_percent_encoded_dot() {
        let err = reject_double_encoding("abc%2e%2e%2fdef", "test").unwrap_err();
        assert!(err.message.contains("%2e"));
    }

    #[test]
    fn accepts_percent_not_followed_by_hex() {
        // "100%" is not a percent-encoded sequence
        assert!(reject_double_encoding("100% done", "test").is_ok());
    }

    // --- JSON payload validation ---

    #[test]
    fn valid_payload_passes() {
        let payload = json!({
            "title": "Test Document",
            "collectionId": "550e8400-e29b-41d4-a716-446655440000",
            "text": "# Hello\n\nWorld"
        });
        let errors = validate_json_payload(&payload, None);
        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn payload_with_control_char_in_title() {
        let payload = json!({
            "title": "Test\x00Document"
        });
        let errors = validate_json_payload(&payload, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].path.contains("title"));
    }

    #[test]
    fn payload_with_hallucinated_id() {
        let payload = json!({
            "id": "abc123?fields=name"
        });
        let errors = validate_json_payload(&payload, None);
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("'?'"));
    }

    #[test]
    fn payload_with_double_encoded_collection_id() {
        let payload = json!({
            "collectionId": "abc%2e%2e%2fdef"
        });
        let errors = validate_json_payload(&payload, None);
        assert!(!errors.is_empty());
    }

    #[test]
    fn payload_nested_array_validation() {
        let payload = json!({
            "dataAttributes": [
                {"dataAttributeId": "valid-looking-id?inject=true", "value": "ok"}
            ]
        });
        let errors = validate_json_payload(&payload, None);
        assert!(!errors.is_empty());
    }

    #[test]
    fn payload_allows_newlines_in_text() {
        let payload = json!({
            "text": "# Header\n\nParagraph 1\n\nParagraph 2\n\n- Item\n- Item\n"
        });
        let errors = validate_json_payload(&payload, None);
        assert!(errors.is_empty());
    }

    // --- Field-aware validation (new tests) ---

    #[test]
    fn content_fields_allow_percent_encoding_patterns() {
        // text, title, description, query fields should allow %, ?, #
        let payload = json!({
            "text": "Visit https://example.com?page=1#section and encode %2F properly",
            "title": "What is 100% uptime?",
            "description": "Use ?query=value for search"
        });
        let errors = validate_json_payload(&payload, None);
        assert!(
            errors.is_empty(),
            "Content fields should allow special chars, got: {errors:?}"
        );
    }

    #[test]
    fn id_fields_still_reject_special_chars() {
        // id and *Id fields should still reject ?, #, %
        let payload = json!({
            "id": "abc?inject=true"
        });
        let errors = validate_json_payload(&payload, None);
        assert!(!errors.is_empty(), "ID fields should reject ?");
    }

    #[test]
    fn schema_driven_uuid_field_gets_strict_validation() {
        // A field with format: uuid in the schema should get strict validation
        // even if its name doesn't end in Id
        let payload = json!({
            "shareId": "abc?inject=true"
        });
        let schema = json!({
            "properties": {
                "shareId": {"type": "string", "format": "uuid"}
            }
        });
        let errors = validate_json_payload(&payload, Some(&schema));
        assert!(
            !errors.is_empty(),
            "UUID format field should get strict validation"
        );
    }

    #[test]
    fn schema_non_uuid_string_allows_special_chars() {
        // A field without format: uuid and not named *Id should allow special chars
        let payload = json!({
            "query": "search for %2e and ? patterns"
        });
        let schema = json!({
            "properties": {
                "query": {"type": "string"}
            }
        });
        let errors = validate_json_payload(&payload, Some(&schema));
        assert!(
            errors.is_empty(),
            "Non-ID string field should allow special chars, got: {errors:?}"
        );
    }

    #[test]
    fn text_field_allows_percent_patterns_with_schema() {
        // Document text with legitimate percent patterns should pass
        let payload = json!({
            "title": "Outline CLI Design Plan",
            "collectionId": "550e8400-e29b-41d4-a716-446655440000",
            "text": "Detect double-encoding: reject %2e sequences in IDs. Example: abc%2Fdef",
            "publish": true
        });
        let schema = json!({
            "properties": {
                "title": {"type": "string"},
                "collectionId": {"type": "string", "format": "uuid"},
                "text": {"type": "string"},
                "publish": {"type": "boolean"}
            }
        });
        let errors = validate_json_payload(&payload, Some(&schema));
        assert!(
            errors.is_empty(),
            "Document text with percent patterns should pass, got: {errors:?}"
        );
    }

    #[test]
    fn allof_schema_resolves_field_types() {
        // allOf composition (used by list/search endpoints) should still resolve fields
        let payload = json!({
            "collectionId": "not-a-uuid?inject=true",
            "query": "search with %2e and ? chars"
        });
        let schema = json!({
            "allOf": [
                {
                    "properties": {
                        "limit": {"type": "number"},
                        "offset": {"type": "number"}
                    }
                },
                {
                    "properties": {
                        "collectionId": {"type": "string", "format": "uuid"},
                        "query": {"type": "string"}
                    }
                }
            ]
        });
        let errors = validate_json_payload(&payload, Some(&schema));
        // collectionId should fail (it's a uuid field with ? in it)
        assert!(
            errors.iter().any(|e| e.path == "collectionId"),
            "collectionId with ? should fail validation"
        );
        // query should pass (it's a content field)
        assert!(
            !errors.iter().any(|e| e.path == "query"),
            "query field should pass validation"
        );
    }

    #[test]
    fn control_chars_rejected_in_all_fields() {
        // Control chars should be rejected everywhere — both ID and content fields
        let payload = json!({
            "text": "Hello\x00World",
            "id": "abc\x07def"
        });
        let errors = validate_json_payload(&payload, None);
        assert!(
            errors.len() >= 2,
            "Both fields should have control char errors"
        );
    }

    // --- is_id_field tests ---

    #[test]
    fn is_id_field_by_name() {
        assert!(is_id_field("id", None));
        assert!(is_id_field("collectionId", None));
        assert!(is_id_field("userId", None));
        assert!(is_id_field("parentDocumentId", None));
        assert!(is_id_field("templateId", None));
        assert!(!is_id_field("text", None));
        assert!(!is_id_field("title", None));
        assert!(!is_id_field("description", None));
        assert!(!is_id_field("query", None));
        assert!(!is_id_field("name", None));
    }

    #[test]
    fn is_id_field_by_schema_format() {
        let uuid_schema = json!({"type": "string", "format": "uuid"});
        let string_schema = json!({"type": "string"});

        // Schema format: uuid overrides name
        assert!(is_id_field("shareId", Some(&uuid_schema)));
        assert!(is_id_field("anyField", Some(&uuid_schema)));

        // Non-uuid schema doesn't make it an ID field (unless name matches)
        assert!(!is_id_field("text", Some(&string_schema)));
        assert!(is_id_field("id", Some(&string_schema))); // name still matches
    }

    // --- Property-based tests (proptest) ---

    proptest! {
        /// Any resource ID containing ?, #, or % must be rejected.
        #[test]
        fn resource_id_rejects_query_params(
            prefix in "[a-zA-Z0-9]{1,20}",
            suffix in "[a-zA-Z0-9]{0,20}"
        ) {
            // With ?
            let id = format!("{prefix}?{suffix}");
            prop_assert!(validate_resource_id(&id).is_err());
            // With #
            let id = format!("{prefix}#{suffix}");
            prop_assert!(validate_resource_id(&id).is_err());
            // With %
            let id = format!("{prefix}%{suffix}");
            prop_assert!(validate_resource_id(&id).is_err());
        }

        /// Control characters below 0x20 (except \n, \r, \t) must always be rejected.
        #[test]
        fn rejects_all_control_chars(
            prefix in "[a-zA-Z ]{0,10}",
            ctrl in (0u8..0x20).prop_filter("not allowed whitespace", |&c| c != b'\n' && c != b'\r' && c != b'\t'),
            suffix in "[a-zA-Z ]{0,10}"
        ) {
            let s = format!("{prefix}{}{suffix}", ctrl as char);
            prop_assert!(reject_control_chars(&s, "test").is_err());
        }

        /// Strings without control chars (>= 0x20) must always be accepted.
        #[test]
        fn accepts_printable_strings(s in "[\\x20-\\x7E]{0,100}") {
            prop_assert!(reject_control_chars(&s, "test").is_ok());
        }

        /// Strings with %XX hex patterns must always be rejected for double-encoding.
        #[test]
        fn rejects_all_percent_hex(
            prefix in "[a-zA-Z]{0,10}",
            h1 in "[0-9a-fA-F]",
            h2 in "[0-9a-fA-F]",
            suffix in "[a-zA-Z]{0,10}"
        ) {
            let s = format!("{prefix}%{h1}{h2}{suffix}");
            prop_assert!(reject_double_encoding(&s, "test").is_err());
        }

        /// Valid UUIDs should always pass resource ID validation.
        #[test]
        fn valid_uuids_accepted(
            a in "[0-9a-f]{8}",
            b in "[0-9a-f]{4}",
            c in "[0-9a-f]{4}",
            d in "[0-9a-f]{4}",
            e in "[0-9a-f]{12}"
        ) {
            let uuid = format!("{a}-{b}-{c}-{d}-{e}");
            prop_assert!(validate_resource_id(&uuid).is_ok());
        }
    }
}
