use serde_json::Value;
use std::collections::HashSet;

/// Resolve all `$ref` pointers in a JSON value against the full OpenAPI spec.
///
/// This walks the value recursively, replacing `{"$ref": "#/components/schemas/Foo"}`
/// with the resolved schema from the spec's `components` section.
///
/// Cycle detection prevents infinite recursion on self-referencing schemas
/// (e.g., NavigationNode.children → NavigationNode).
pub fn resolve_refs(value: &Value, spec: &Value) -> Value {
    let mut visited = HashSet::new();
    resolve_recursive(value, spec, &mut visited)
}

fn resolve_recursive(value: &Value, spec: &Value, visited: &mut HashSet<String>) -> Value {
    match value {
        Value::Object(obj) => {
            // Check if this is a $ref object
            if let Some(ref_str) = obj.get("$ref").and_then(|v| v.as_str()) {
                if visited.contains(ref_str) {
                    // Cycle detected: return a placeholder instead of recursing
                    return serde_json::json!({
                        "$circular_ref": ref_str,
                        "description": "Circular reference detected, not expanded"
                    });
                }

                // Resolve the ref
                if let Some(resolved) = resolve_ref_path(ref_str, spec) {
                    visited.insert(ref_str.to_string());
                    let result = resolve_recursive(&resolved, spec, visited);
                    visited.remove(ref_str);
                    return result;
                }

                // Ref couldn't be resolved — return as-is
                return value.clone();
            }

            // Not a $ref — recurse into all properties
            let mut result = serde_json::Map::new();
            for (key, val) in obj {
                result.insert(key.clone(), resolve_recursive(val, spec, visited));
            }
            Value::Object(result)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| resolve_recursive(v, spec, visited))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// Resolve a JSON Pointer-style `$ref` path against the spec.
///
/// Input: "#/components/schemas/Document"
/// Walks: spec["components"]["schemas"]["Document"]
fn resolve_ref_path(ref_path: &str, spec: &Value) -> Option<Value> {
    // Only handle internal refs (#/...)
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

    #[test]
    fn resolves_simple_ref() {
        let spec = json!({
            "components": {
                "schemas": {
                    "Foo": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"}
                        }
                    }
                }
            }
        });

        let input = json!({"$ref": "#/components/schemas/Foo"});
        let resolved = resolve_refs(&input, &spec);

        assert_eq!(resolved["type"], "object");
        assert_eq!(resolved["properties"]["name"]["type"], "string");
    }

    #[test]
    fn resolves_nested_refs() {
        let spec = json!({
            "components": {
                "schemas": {
                    "User": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"}
                        }
                    },
                    "Document": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "createdBy": {"$ref": "#/components/schemas/User"}
                        }
                    }
                }
            }
        });

        let input = json!({"$ref": "#/components/schemas/Document"});
        let resolved = resolve_refs(&input, &spec);

        assert_eq!(resolved["properties"]["title"]["type"], "string");
        assert_eq!(
            resolved["properties"]["createdBy"]["properties"]["name"]["type"],
            "string"
        );
    }

    #[test]
    fn handles_circular_refs() {
        let spec = json!({
            "components": {
                "schemas": {
                    "NavigationNode": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "children": {
                                "type": "array",
                                "items": {"$ref": "#/components/schemas/NavigationNode"}
                            }
                        }
                    }
                }
            }
        });

        let input = json!({"$ref": "#/components/schemas/NavigationNode"});
        let resolved = resolve_refs(&input, &spec);

        // First level should be resolved
        assert_eq!(resolved["properties"]["title"]["type"], "string");
        // Second level should detect the cycle
        assert!(resolved["properties"]["children"]["items"]["$circular_ref"].is_string());
    }

    #[test]
    fn resolves_allof_with_refs() {
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

        let input = json!({
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

        let resolved = resolve_refs(&input, &spec);
        let all_of = resolved["allOf"].as_array().unwrap();
        // The $ref in allOf should be resolved
        assert_eq!(all_of[0]["properties"]["offset"]["type"], "number");
        // The inline object should pass through unchanged
        assert_eq!(all_of[1]["properties"]["collectionId"]["format"], "uuid");
    }

    #[test]
    fn unresolvable_ref_passes_through() {
        let spec = json!({});
        let input = json!({"$ref": "#/components/schemas/NonExistent"});
        let resolved = resolve_refs(&input, &spec);
        assert_eq!(resolved["$ref"], "#/components/schemas/NonExistent");
    }

    #[test]
    fn resolves_real_spec_document_schema() {
        let spec_json = include_str!("../../api/spec3.json");
        let spec: Value = serde_json::from_str(spec_json).unwrap();

        let doc_ref = json!({"$ref": "#/components/schemas/Document"});
        let resolved = resolve_refs(&doc_ref, &spec);

        // Should have resolved the Document schema
        assert_eq!(resolved["type"], "object");
        assert!(resolved["properties"]["id"].is_object());
        assert!(resolved["properties"]["title"].is_object());
        // createdBy should be resolved (User schema)
        assert!(resolved["properties"]["createdBy"]["properties"]["name"].is_object());
    }
}
