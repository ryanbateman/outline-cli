//! MCP (Model Context Protocol) server for the Outline CLI.
//!
//! Exposes Outline API methods as MCP tools over stdio, allowing agent
//! frameworks (Claude Code, Cursor, etc.) to invoke the API without
//! shell escaping issues.
//!
//! The tool list is generated dynamically from the same OpenAPI spec
//! that powers the CLI — same source of truth, same validation.

use std::collections::HashSet;
use std::sync::Arc;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, Implementation, InitializeResult,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool,
};
use rmcp::service::RequestContext;
use rmcp::service::RoleServer;
use rmcp::{ServiceExt, transport::io::stdio};
use serde_json::Value;

use crate::auth::Credentials;
use crate::executor::execute_request;
use crate::spec::{ApiSpec, resolve_refs, validate_payload};
use crate::validate::validate_json_payload;

/// MCP server that dynamically exposes Outline API methods as tools.
pub struct OutlineServer {
    /// Parsed OpenAPI spec (source of truth for tools).
    api_spec: Arc<ApiSpec>,
    /// Credentials for API calls.
    credentials: Credentials,
    /// Set of exposed resource names (empty = expose all).
    exposed: HashSet<String>,
}

impl OutlineServer {
    pub fn new(api_spec: Arc<ApiSpec>, credentials: Credentials, expose: Option<&str>) -> Self {
        let exposed = match expose {
            Some(list) => list.split(',').map(|s| s.trim().to_string()).collect(),
            None => HashSet::new(),
        };
        Self {
            api_spec,
            credentials,
            exposed,
        }
    }

    /// Check if a resource should be exposed.
    fn is_exposed(&self, resource: &str) -> bool {
        self.exposed.is_empty() || self.exposed.contains(resource)
    }

    /// Build the list of MCP tools from the API spec.
    fn build_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();

        for resource_name in self.api_spec.resource_names() {
            if !self.is_exposed(resource_name) {
                continue;
            }

            let methods = match self.api_spec.methods(resource_name) {
                Some(m) => m,
                None => continue,
            };

            for method in methods {
                let tool_name = format!("{}.{}", resource_name, method.action);

                // Build input schema from the OpenAPI request body schema.
                // Resolve $refs so agents get complete schemas.
                let input_schema = if let Some(ref schema) = method.request_schema {
                    let resolved = resolve_refs(schema, &self.api_spec.raw);
                    match resolved {
                        Value::Object(map) => map,
                        _ => default_input_schema(),
                    }
                } else {
                    default_input_schema()
                };

                let tool = Tool::new(tool_name, method.summary.clone(), input_schema);

                tools.push(tool);
            }
        }

        tools
    }
}

/// Default empty object schema for methods without request bodies.
fn default_input_schema() -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert("type".to_string(), Value::String("object".to_string()));
    map.insert(
        "properties".to_string(),
        Value::Object(serde_json::Map::new()),
    );
    map
}

impl ServerHandler for OutlineServer {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_server_info(Implementation::new(
            "outline-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Outline knowledge base MCP server. \
             Use tools/list to discover available API methods. \
             Pass arguments as the API request body (same as --json in CLI mode). \
             All methods are POST. Use --fields equivalent by requesting specific fields in your prompt."
                .to_string(),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::model::ErrorData> {
        let tools = self.build_tools();
        Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        // Parse "resource.action" format
        let (resource, action) = name.split_once('.')?;

        if !self.is_exposed(resource) {
            return None;
        }

        let method = self.api_spec.find_method(resource, action)?;

        let input_schema = if let Some(ref schema) = method.request_schema {
            let resolved = resolve_refs(schema, &self.api_spec.raw);
            match resolved {
                Value::Object(map) => map,
                _ => default_input_schema(),
            }
        } else {
            default_input_schema()
        };

        Some(Tool::new(
            name.to_string(),
            method.summary.clone(),
            input_schema,
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::model::ErrorData> {
        let tool_name = request.name.to_string();

        // Parse "resource.action" format
        let (resource, action) = match tool_name.split_once('.') {
            Some(s) => s,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid tool name '{}'. Expected 'resource.action' format (e.g., documents.create)",
                    tool_name
                ))]));
            }
        };

        // Check if resource is exposed
        if !self.is_exposed(resource) {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Resource '{}' is not exposed. Use --expose to enable it.",
                resource
            ))]));
        }

        // Look up method in spec
        let method = match self.api_spec.find_method(resource, action) {
            Some(m) => m,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Unknown method: {}.{}",
                    resource, action
                ))]));
            }
        };

        // Convert arguments to Value
        let body: Option<Value> = request.arguments.map(Value::Object);

        // Input validation (same as CLI, field-aware)
        if let Some(ref payload) = body {
            let input_errors = validate_json_payload(payload, method.request_schema.as_ref());
            if !input_errors.is_empty() {
                let messages: Vec<String> = input_errors.iter().map(|e| e.to_string()).collect();
                let error_response = serde_json::json!({
                    "ok": false,
                    "error": "input_validation_error",
                    "message": messages.join("; "),
                    "details": messages,
                });
                return Ok(CallToolResult::error(vec![Content::text(
                    serde_json::to_string_pretty(&error_response).unwrap_or_default(),
                )]));
            }
        }

        // Schema validation (same as CLI)
        if let Some(ref payload) = body
            && let Some(ref request_schema) = method.request_schema
        {
            let schema_errors = validate_payload(payload, request_schema, &self.api_spec.raw);
            if !schema_errors.is_empty() {
                let messages: Vec<String> = schema_errors.iter().map(|e| e.to_string()).collect();
                let error_response = serde_json::json!({
                    "ok": false,
                    "error": "schema_validation_error",
                    "message": messages.join("; "),
                    "details": messages,
                });
                return Ok(CallToolResult::error(vec![Content::text(
                    serde_json::to_string_pretty(&error_response).unwrap_or_default(),
                )]));
            }
        }

        // Execute the API request
        let response = match execute_request(&self.credentials, &method.path, body.as_ref()).await {
            Ok(r) => r,
            Err(e) => {
                let error_response = serde_json::json!({
                    "ok": false,
                    "error": "request_failed",
                    "message": format!("{e:#}"),
                });
                return Ok(CallToolResult::error(vec![Content::text(
                    serde_json::to_string_pretty(&error_response).unwrap_or_default(),
                )]));
            }
        };

        // Return the response as JSON text content
        let is_error =
            !response.is_success() || response.body.get("ok") == Some(&Value::Bool(false));

        let response_text = serde_json::to_string_pretty(&response.body)
            .unwrap_or_else(|_| response.body.to_string());

        if is_error {
            Ok(CallToolResult::error(vec![Content::text(response_text)]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(response_text)]))
        }
    }
}

/// Start the MCP server over stdio.
///
/// This blocks until the client disconnects.
pub async fn run_mcp_server(
    api_spec: Arc<ApiSpec>,
    credentials: Credentials,
    expose: Option<&str>,
) -> anyhow::Result<()> {
    let server = OutlineServer::new(api_spec, credentials, expose);

    let service = server
        .serve(stdio())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start MCP server: {e}"))?;

    service
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_spec;

    fn test_spec() -> ApiSpec {
        let json = include_str!("../../api/spec3.json");
        parse_spec(json).expect("spec should parse")
    }

    fn test_credentials() -> Credentials {
        Credentials {
            api_token: "ol_api_testtoken1234567890abcdefghijklmnop".to_string(),
            api_url: "https://example.com/api".to_string(),
        }
    }

    #[test]
    fn build_tools_exposes_all_when_no_filter() {
        let spec = test_spec();
        let server = OutlineServer::new(Arc::new(spec.clone()), test_credentials(), None);
        let tools = server.build_tools();

        // Should have tools for all methods
        assert!(!tools.is_empty());

        // Verify tool naming convention
        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&"documents.create".to_string()));
        assert!(names.contains(&"documents.list".to_string()));
        assert!(names.contains(&"collections.list".to_string()));
    }

    #[test]
    fn build_tools_filters_by_expose() {
        let spec = test_spec();
        let server = OutlineServer::new(
            Arc::new(spec.clone()),
            test_credentials(),
            Some("documents"),
        );
        let tools = server.build_tools();

        // Should only have document tools
        for tool in &tools {
            assert!(
                tool.name.starts_with("documents."),
                "Expected only documents tools, got: {}",
                tool.name
            );
        }
    }

    #[test]
    fn build_tools_multiple_expose() {
        let spec = test_spec();
        let server = OutlineServer::new(
            Arc::new(spec.clone()),
            test_credentials(),
            Some("documents,collections"),
        );
        let tools = server.build_tools();

        for tool in &tools {
            assert!(
                tool.name.starts_with("documents.") || tool.name.starts_with("collections."),
                "Expected only documents/collections tools, got: {}",
                tool.name
            );
        }
    }

    #[test]
    fn tools_have_input_schemas() {
        let spec = test_spec();
        let server = OutlineServer::new(Arc::new(spec), test_credentials(), Some("documents"));
        let tools = server.build_tools();

        for tool in &tools {
            // Every tool should have a non-empty input schema.
            // Schemas may have "type"+"properties" (simple object),
            // "allOf" (composed), or other valid JSON Schema structures.
            assert!(
                !tool.input_schema.is_empty(),
                "Tool {} has empty input schema",
                tool.name
            );
        }
    }

    #[test]
    fn get_tool_returns_known_tool() {
        let spec = test_spec();
        let server = OutlineServer::new(Arc::new(spec), test_credentials(), None);

        let tool = server.get_tool("documents.create");
        assert!(tool.is_some(), "Should find documents.create");
        let tool = tool.unwrap();
        assert_eq!(tool.name, "documents.create");
    }

    #[test]
    fn get_tool_returns_none_for_unknown() {
        let spec = test_spec();
        let server = OutlineServer::new(Arc::new(spec), test_credentials(), None);

        assert!(server.get_tool("nonexistent.method").is_none());
        assert!(server.get_tool("invalid-format").is_none());
    }

    #[test]
    fn get_tool_respects_expose_filter() {
        let spec = test_spec();
        let server = OutlineServer::new(Arc::new(spec), test_credentials(), Some("collections"));

        // Documents should be filtered out
        assert!(server.get_tool("documents.create").is_none());
        // Collections should be available
        assert!(server.get_tool("collections.list").is_some());
    }

    #[test]
    fn get_info_returns_valid_server_info() {
        let spec = test_spec();
        let server = OutlineServer::new(Arc::new(spec), test_credentials(), None);
        let info = server.get_info();
        assert_eq!(info.server_info.name, "outline-mcp");
        assert!(info.instructions.is_some());
    }
}
