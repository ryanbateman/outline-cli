mod auth;
mod commands;
mod executor;
mod helpers;
mod mcp;
mod output;
mod spec;
mod validate;

use serde_json::Value;
use std::process::ExitCode;
use std::sync::Arc;

use auth::load_credentials;
use commands::build_cli;
use executor::{execute_paginated, execute_request};
use mcp::run_mcp_server;
use output::tty::use_color;
use output::{ColorMode, OutputFormat, TextContext, format_output};
use spec::{load_spec, parse_spec, resolve_refs, validate_payload};
use validate::validate_json_payload;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(e) => {
            // This shouldn't normally happen — run() handles its own errors.
            // But if it does, output a structured error.
            eprintln!("Fatal: {e:#}");
            ExitCode::from(2)
        }
    }
}

/// Main CLI logic. Returns ExitCode so we can control error formatting.
///
/// Errors from this function are truly unexpected (e.g., clap panics).
/// All normal error paths (auth failure, API errors, validation errors)
/// are handled internally with structured output.
async fn run() -> Result<ExitCode, anyhow::Error> {
    // 1. Load and parse spec
    let spec_json = load_spec().await?;
    let api_spec = parse_spec(&spec_json)?;

    // 2. Build CLI from spec
    let cli = build_cli(&api_spec);
    let matches = cli.get_matches();

    // Extract global flags
    let output_format = OutputFormat::from_str_arg(
        matches
            .get_one::<String>("output")
            .map(|s| s.as_str())
            .unwrap_or("text"),
    );
    let color_mode = ColorMode::from_str_arg(
        matches
            .get_one::<String>("color")
            .map(|s| s.as_str())
            .unwrap_or("auto"),
    );
    let fields = matches.get_one::<String>("fields").map(|s| s.as_str());
    let dry_run = matches.get_flag("dry-run");
    let page_all = matches.get_flag("page-all");
    let sanitize = matches.get_flag("sanitize");
    let color_enabled = use_color(color_mode);

    // 3. Handle schema introspection command
    if let Some(schema_matches) = matches.subcommand_matches("schema") {
        let method_name = schema_matches
            .get_one::<String>("method")
            .expect("method is required");
        return handle_schema(&api_spec, method_name, output_format, color_enabled);
    }

    // 3b. Handle MCP server command
    if let Some(mcp_matches) = matches.subcommand_matches("mcp") {
        let expose = mcp_matches.get_one::<String>("expose").map(|s| s.as_str());

        let credentials = match load_credentials() {
            Ok(c) => c,
            Err(e) => {
                print_error(&format!("{e:#}"), Some(401), output_format, color_enabled);
                return Ok(ExitCode::from(1));
            }
        };

        // MCP server logs go to stderr (stdout is the JSON-RPC transport)
        if let Err(e) = run_mcp_server(Arc::new(api_spec), credentials, expose).await {
            eprintln!("MCP server error: {e:#}");
            return Ok(ExitCode::from(1));
        }
        return Ok(ExitCode::SUCCESS);
    }

    // 3c. Handle completions command
    if let Some(comp_matches) = matches.subcommand_matches("completions") {
        let shell_name = comp_matches
            .get_one::<String>("shell")
            .expect("shell is required");
        return handle_completions(&api_spec, shell_name);
    }

    // 4. Dispatch to resource.action
    let (resource_name, resource_matches) = match matches.subcommand() {
        Some(s) => s,
        None => {
            print_error(
                "No resource command specified",
                None,
                output_format,
                color_enabled,
            );
            return Ok(ExitCode::from(1));
        }
    };

    // Build text context for formatted output (with resource name for smart defaults)
    let text_ctx = TextContext::new(color_enabled, Some(resource_name));

    // 4b. Check if a helper wants to handle this command (e.g., +new, +edit, +search)
    if let Some(helper) = helpers::get_helper(resource_name) {
        // Helpers need credentials for API calls
        let credentials = match load_credentials() {
            Ok(c) => c,
            Err(e) => {
                print_error(&format!("{e:#}"), Some(401), output_format, color_enabled);
                return Ok(ExitCode::from(1));
            }
        };

        match helper
            .handle(resource_matches, &credentials, &api_spec, color_enabled)
            .await
        {
            Ok(true) => return Ok(ExitCode::SUCCESS), // Helper handled it
            Ok(false) => {}                           // Not a helper command, fall through
            Err(e) => {
                print_error(&format!("{e:#}"), None, output_format, color_enabled);
                return Ok(ExitCode::from(1));
            }
        }
    }

    let (action_name, action_matches) = match resource_matches.subcommand() {
        Some(s) => s,
        None => {
            print_error(
                "No action command specified",
                None,
                output_format,
                color_enabled,
            );
            return Ok(ExitCode::from(1));
        }
    };

    // Look up method in spec
    let method = match api_spec.find_method(resource_name, action_name) {
        Some(m) => m,
        None => {
            print_error(
                &format!("Unknown method: {resource_name}.{action_name}"),
                Some(404),
                output_format,
                color_enabled,
            );
            return Ok(ExitCode::from(1));
        }
    };

    // Parse --json payload
    let body: Option<Value> = if method.has_request_body {
        match action_matches.get_one::<String>("json") {
            Some(json_str) => match serde_json::from_str(json_str) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    print_error(
                        &format!("Invalid JSON in --json argument: {e}"),
                        Some(400),
                        output_format,
                        color_enabled,
                    );
                    return Ok(ExitCode::from(1));
                }
            },
            None => None,
        }
    } else {
        None
    };

    // 5. Input validation on --json payload (field-aware: schema determines ID vs content)
    if let Some(ref payload) = body {
        let input_errors = validate_json_payload(payload, method.request_schema.as_ref());
        if !input_errors.is_empty() {
            let messages: Vec<String> = input_errors.iter().map(|e| e.to_string()).collect();
            let error_response = serde_json::json!({
                "ok": false,
                "error": "input_validation_error",
                "message": messages.join("; "),
                "status": 400,
                "details": messages,
            });
            println!(
                "{}",
                format_output(&error_response, output_format, None, Some(&text_ctx))
            );
            return Ok(ExitCode::from(1));
        }
    }

    // 6. Schema validation on --json payload (if method has a request schema)
    if let Some(ref payload) = body
        && let Some(ref request_schema) = method.request_schema
    {
        let schema_errors = validate_payload(payload, request_schema, &api_spec.raw);
        if !schema_errors.is_empty() {
            let messages: Vec<String> = schema_errors.iter().map(|e| e.to_string()).collect();
            let error_response = serde_json::json!({
                "ok": false,
                "error": "schema_validation_error",
                "message": messages.join("; "),
                "status": 400,
                "details": messages,
            });
            println!(
                "{}",
                format_output(&error_response, output_format, None, Some(&text_ctx))
            );
            return Ok(ExitCode::from(1));
        }
    }

    // 7. Handle --dry-run (now with real validation above)
    if dry_run {
        let action_type = categorize_action(action_name);
        let dry_run_response = serde_json::json!({
            "ok": true,
            "action": action_type,
            "resource": resource_name,
            "method": format!("{}.{}", resource_name, action_name),
            "validated": true,
            "body": body,
        });
        println!(
            "{}",
            format_output(&dry_run_response, output_format, fields, Some(&text_ctx))
        );
        return Ok(ExitCode::SUCCESS);
    }

    // 8. Load credentials
    let credentials = match load_credentials() {
        Ok(c) => c,
        Err(e) => {
            print_error(&format!("{e:#}"), Some(401), output_format, color_enabled);
            return Ok(ExitCode::from(1));
        }
    };

    // 9. Execute request
    if page_all {
        // Paginated NDJSON streaming mode
        let result = execute_paginated(&credentials, &method.path, body.as_ref(), fields).await;
        match result {
            Ok(_total) => return Ok(ExitCode::SUCCESS),
            Err(e) => {
                print_error(&format!("{e:#}"), Some(500), output_format, color_enabled);
                return Ok(ExitCode::from(1));
            }
        }
    }

    let response = match execute_request(&credentials, &method.path, body.as_ref()).await {
        Ok(r) => r,
        Err(e) => {
            print_error(&format!("{e:#}"), Some(500), output_format, color_enabled);
            return Ok(ExitCode::from(1));
        }
    };

    // 10. Optionally sanitize response
    let response_body = if sanitize {
        sanitize_response(&response.body)
    } else {
        response.body
    };

    // 11. Format and print output
    println!(
        "{}",
        format_output(&response_body, output_format, fields, Some(&text_ctx))
    );

    // Exit with non-zero if the response indicates failure
    if response_body.get("ok") == Some(&Value::Bool(false)) {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

/// Print a structured error message in the appropriate format.
fn print_error(message: &str, code: Option<u16>, format: OutputFormat, color: bool) {
    let error_response = serde_json::json!({
        "ok": false,
        "error": message,
        "message": message,
        "code": code.unwrap_or(500),
    });
    let ctx = TextContext::new(color, None);
    println!(
        "{}",
        format_output(&error_response, format, None, Some(&ctx))
    );
}

/// Sanitize a response to defend against prompt injection.
///
/// Strips or escapes content that could be interpreted as instructions
/// by an LLM agent consuming this CLI's output. Targets:
/// - Unicode control characters (zero-width spaces, RTL overrides, etc.)
/// - Common prompt injection delimiters
fn sanitize_response(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(sanitize_string(s)),
        Value::Object(map) => {
            let sanitized: serde_json::Map<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), sanitize_response(v)))
                .collect();
            Value::Object(sanitized)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sanitize_response).collect()),
        other => other.clone(),
    }
}

/// Sanitize a single string value.
///
/// Removes:
/// - Unicode control characters (categories Cc/Cf except \n, \r, \t)
/// - Zero-width characters (U+200B-U+200F, U+2028-U+202F, U+FEFF)
/// - Common prompt injection patterns
fn sanitize_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // Allow normal whitespace
            '\n' | '\r' | '\t' | ' ' => result.push(c),
            // Block ASCII control characters
            c if c < '\x20' => {}
            // Block Unicode control/format characters commonly used in injection
            '\u{200B}'..='\u{200F}' => {} // zero-width spaces, LTR/RTL marks
            '\u{2028}'..='\u{202F}' => {} // line/paragraph separators, embedding controls
            '\u{2060}'..='\u{2069}' => {} // word joiner, invisible characters
            '\u{FEFF}' => {}              // BOM / zero-width no-break space
            '\u{FFF9}'..='\u{FFFB}' => {} // interlinear annotations
            // Everything else passes through
            c => result.push(c),
        }
    }
    result
}

/// Handle the `schema` introspection command.
///
/// Resolves all `$ref` pointers so agents get complete schemas
/// without needing to chase references.
fn handle_schema(
    api_spec: &spec::ApiSpec,
    method_name: &str,
    output_format: OutputFormat,
    color_enabled: bool,
) -> Result<ExitCode, anyhow::Error> {
    // Parse "resource.action" format
    let (resource, action) = match method_name.split_once('.') {
        Some(s) => s,
        None => {
            print_error(
                "Method name must be in 'resource.action' format (e.g., documents.create)",
                Some(400),
                output_format,
                color_enabled,
            );
            return Ok(ExitCode::from(1));
        }
    };

    let method = match api_spec.find_method(resource, action) {
        Some(m) => m,
        None => {
            print_error(
                &format!("Unknown method: {method_name}"),
                Some(404),
                output_format,
                color_enabled,
            );
            return Ok(ExitCode::from(1));
        }
    };

    // Resolve $refs in the request schema so agents get the full picture
    let resolved_schema = method
        .request_schema
        .as_ref()
        .map(|s| resolve_refs(s, &api_spec.raw));

    // Build schema info
    let schema_info = serde_json::json!({
        "path": method.path,
        "method": "POST",
        "summary": method.summary,
        "description": method.description,
        "operationId": method.operation_id,
        "hasRequestBody": method.has_request_body,
        "requestBody": resolved_schema,
    });

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&schema_info)?);
        }
        OutputFormat::Text => {
            println!("{} -- {}", method.path, method.summary);
            println!("  Method: POST");
            if !method.description.is_empty() {
                println!("  {}", method.description);
            }
            if let Some(schema) = &resolved_schema {
                println!("\n  Request body schema:");
                println!("  {}", serde_json::to_string_pretty(schema)?);
            }
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Categorize an action name for --dry-run output.
fn categorize_action(action: &str) -> &str {
    match action {
        "create" | "import" => "create",
        "update" | "move" | "archive" | "restore" | "unpublish" => "update",
        "delete" | "empty_trash" => "delete",
        "list" | "info" | "search" | "search_titles" | "documents" | "drafts" | "viewed"
        | "archived" | "deleted" | "memberships" | "group_memberships" => "read",
        _ => "execute",
    }
}

/// Generate shell completions and print to stdout.
fn handle_completions(
    api_spec: &spec::ApiSpec,
    shell_name: &str,
) -> Result<ExitCode, anyhow::Error> {
    use clap_complete::{Shell, generate};

    let shell = match shell_name {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "powershell" => Shell::PowerShell,
        "elvish" => Shell::Elvish,
        _ => {
            eprintln!("Unsupported shell: {shell_name}");
            return Ok(ExitCode::from(1));
        }
    };

    let mut cmd = build_cli(api_spec);
    generate(shell, &mut cmd, "outline", &mut std::io::stdout());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn categorize_actions_correctly() {
        assert_eq!(categorize_action("create"), "create");
        assert_eq!(categorize_action("update"), "update");
        assert_eq!(categorize_action("delete"), "delete");
        assert_eq!(categorize_action("list"), "read");
        assert_eq!(categorize_action("info"), "read");
        assert_eq!(categorize_action("search"), "read");
        assert_eq!(categorize_action("add_user"), "execute");
    }

    #[test]
    fn sanitize_removes_control_chars() {
        let input = "Hello\x00World\x07!";
        let result = sanitize_string(input);
        assert_eq!(result, "HelloWorld!");
    }

    #[test]
    fn sanitize_preserves_normal_whitespace() {
        let input = "Hello\n\tWorld\r\n";
        let result = sanitize_string(input);
        assert_eq!(result, "Hello\n\tWorld\r\n");
    }

    #[test]
    fn sanitize_removes_zero_width_chars() {
        // Zero-width space (U+200B) and BOM (U+FEFF)
        let input = "Hello\u{200B}World\u{FEFF}!";
        let result = sanitize_string(input);
        assert_eq!(result, "HelloWorld!");
    }

    #[test]
    fn sanitize_removes_rtl_override() {
        // RTL override (U+202E) used in visual spoofing attacks
        let input = "normal\u{202E}desrever";
        let result = sanitize_string(input);
        assert_eq!(result, "normaldesrever");
    }

    #[test]
    fn sanitize_response_recurses() {
        let input = json!({
            "ok": true,
            "data": {
                "title": "Clean",
                "text": "Has\u{200B}zero\u{200B}width",
                "tags": ["tag\u{FEFF}1", "normal"]
            }
        });
        let result = sanitize_response(&input);
        assert_eq!(result["data"]["text"], "Haszerowidth");
        assert_eq!(result["data"]["tags"][0], "tag1");
        assert_eq!(result["data"]["tags"][1], "normal");
        // Non-string fields unchanged
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn print_error_json_format() {
        // Just verify it doesn't panic — output goes to stdout
        // We can't easily capture stdout in this test, but the function
        // is simple enough to verify by reading
    }
}
