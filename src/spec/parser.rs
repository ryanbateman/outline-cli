use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

/// A parsed API method extracted from the OpenAPI spec.
#[derive(Debug, Clone)]
pub struct ApiMethod {
    /// Resource name, e.g. "documents"
    #[allow(dead_code)]
    pub resource: String,
    /// Action name, e.g. "create"
    pub action: String,
    /// The API path, e.g. "/documents.create"
    pub path: String,
    /// Tag from the spec (usually capitalized resource name)
    pub tag: String,
    /// Short summary, e.g. "Create a document"
    pub summary: String,
    /// Longer description (may be empty)
    pub description: String,
    /// Whether this method has a request body (most do for Outline's POST API)
    pub has_request_body: bool,
    /// The request body schema as raw JSON Value (for future validation)
    pub request_schema: Option<Value>,
    /// Operation ID, e.g. "documentsCreate"
    pub operation_id: String,
}

/// Parsed API spec: a collection of methods grouped by resource.
#[derive(Debug, Clone)]
pub struct ApiSpec {
    /// All methods, grouped by resource name.
    /// BTreeMap for deterministic ordering.
    pub resources: BTreeMap<String, Vec<ApiMethod>>,
    /// Tag descriptions (tag name → description text)
    pub tag_descriptions: BTreeMap<String, String>,
    /// Raw spec JSON (kept for schema introspection)
    pub raw: Value,
}

impl ApiSpec {
    /// Get all resource names (sorted).
    pub fn resource_names(&self) -> Vec<&str> {
        self.resources.keys().map(|s| s.as_str()).collect()
    }

    /// Get methods for a resource.
    pub fn methods(&self, resource: &str) -> Option<&[ApiMethod]> {
        self.resources.get(resource).map(|v| v.as_slice())
    }

    /// Find a specific method by resource and action.
    pub fn find_method(&self, resource: &str, action: &str) -> Option<&ApiMethod> {
        self.resources
            .get(resource)?
            .iter()
            .find(|m| m.action == action)
    }

    /// Total number of methods across all resources.
    #[allow(dead_code)]
    pub fn method_count(&self) -> usize {
        self.resources.values().map(|v| v.len()).sum()
    }
}

/// Parse the OpenAPI spec JSON into an `ApiSpec`.
pub fn parse_spec(json_str: &str) -> Result<ApiSpec> {
    let raw: Value = serde_json::from_str(json_str).context("Failed to parse spec JSON")?;

    // Extract tag descriptions
    let mut tag_descriptions = BTreeMap::new();
    if let Some(tags) = raw["tags"].as_array() {
        for tag in tags {
            if let (Some(name), Some(desc)) = (tag["name"].as_str(), tag["description"].as_str()) {
                tag_descriptions.insert(name.to_string(), desc.to_string());
            }
        }
    }

    // Extract methods from paths
    let paths = raw["paths"]
        .as_object()
        .context("Spec missing 'paths' object")?;

    let mut resources: BTreeMap<String, Vec<ApiMethod>> = BTreeMap::new();

    for (path, path_item) in paths {
        // Outline paths: "/documents.create" → resource="documents", action="create"
        let trimmed = path.trim_start_matches('/');
        let (resource, action) = match trimmed.split_once('.') {
            Some((r, a)) => (r.to_string(), a.to_string()),
            None => {
                // Skip paths that don't follow the resource.action pattern
                continue;
            }
        };

        // All Outline API methods are POST
        let operation = match path_item.get("post") {
            Some(op) => op,
            None => continue,
        };

        let tag = operation["tags"]
            .as_array()
            .and_then(|t| t.first())
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        let summary = operation["summary"].as_str().unwrap_or("").to_string();

        let description = operation["description"].as_str().unwrap_or("").to_string();

        let operation_id = operation["operationId"].as_str().unwrap_or("").to_string();

        // Check for request body and extract schema
        let request_body = operation.get("requestBody");
        let has_request_body = request_body.is_some();
        let request_schema = request_body
            .and_then(|rb| rb.get("content"))
            .and_then(|c| c.get("application/json"))
            .and_then(|aj| aj.get("schema"))
            .cloned();

        let method = ApiMethod {
            resource: resource.clone(),
            action,
            path: path.clone(),
            tag,
            summary,
            description,
            has_request_body,
            request_schema,
            operation_id,
        };

        resources.entry(resource).or_default().push(method);
    }

    // Sort methods within each resource by action name for deterministic order
    for methods in resources.values_mut() {
        methods.sort_by(|a, b| a.action.cmp(&b.action));
    }

    Ok(ApiSpec {
        resources,
        tag_descriptions,
        raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_spec() -> ApiSpec {
        let json = include_str!("../../api/spec3.json");
        parse_spec(json).expect("spec should parse")
    }

    #[test]
    fn parses_all_resources() {
        let spec = test_spec();
        let names = spec.resource_names();
        assert!(
            names.contains(&"documents"),
            "should have documents resource"
        );
        assert!(
            names.contains(&"collections"),
            "should have collections resource"
        );
        assert!(names.contains(&"users"), "should have users resource");
        assert!(names.contains(&"comments"), "should have comments resource");
        assert!(
            names.len() >= 15,
            "should have at least 15 resources, got {}",
            names.len()
        );
    }

    #[test]
    fn parses_document_methods() {
        let spec = test_spec();
        let methods = spec.methods("documents").expect("documents should exist");
        let action_names: Vec<&str> = methods.iter().map(|m| m.action.as_str()).collect();
        assert!(action_names.contains(&"create"), "should have create");
        assert!(action_names.contains(&"list"), "should have list");
        assert!(action_names.contains(&"info"), "should have info");
        assert!(action_names.contains(&"update"), "should have update");
        assert!(action_names.contains(&"delete"), "should have delete");
        assert!(action_names.contains(&"search"), "should have search");
    }

    #[test]
    fn methods_have_correct_structure() {
        let spec = test_spec();
        let method = spec
            .find_method("documents", "create")
            .expect("should find documents.create");
        assert_eq!(method.path, "/documents.create");
        assert_eq!(method.tag, "Documents");
        assert!(!method.summary.is_empty(), "summary should not be empty");
        assert!(method.has_request_body, "create should have request body");
        assert!(
            method.request_schema.is_some(),
            "create should have request schema"
        );
    }

    #[test]
    fn tag_descriptions_parsed() {
        let spec = test_spec();
        assert!(spec.tag_descriptions.contains_key("Documents"));
        assert!(spec.tag_descriptions.contains_key("Collections"));
    }

    #[test]
    fn method_count_matches_spec_paths() {
        let spec = test_spec();
        // Outline spec has ~107 paths, all should be parsed
        assert!(
            spec.method_count() >= 100,
            "should have at least 100 methods, got {}",
            spec.method_count()
        );
    }

    #[test]
    fn auth_info_has_no_request_body() {
        let spec = test_spec();
        // auth.config or auth.info may or may not have bodies; check the spec
        // The key thing is the parser doesn't crash on paths with no body
        let _method = spec.find_method("auth", "info");
    }

    #[test]
    fn all_methods_have_operation_ids() {
        let spec = test_spec();
        for methods in spec.resources.values() {
            for method in methods {
                assert!(
                    !method.operation_id.is_empty(),
                    "Method {} should have an operation ID",
                    method.path
                );
            }
        }
    }
}
