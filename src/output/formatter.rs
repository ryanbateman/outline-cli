use serde_json::Value;

use super::color;
use super::table::Table;
use super::tty;

/// Output format for CLI responses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Json,
    Text,
}

impl OutputFormat {
    pub fn from_str_arg(s: &str) -> Self {
        match s {
            "json" => OutputFormat::Json,
            _ => OutputFormat::Text,
        }
    }
}

/// Context for text formatting: color state, terminal width, resource name.
pub struct TextContext {
    pub color: bool,
    pub width: usize,
    pub resource: Option<String>,
}

impl TextContext {
    pub fn new(color: bool, resource: Option<&str>) -> Self {
        Self {
            color,
            width: tty::terminal_width(),
            resource: resource.map(|s| s.to_string()),
        }
    }

    /// For tests: create a context with explicit width.
    #[cfg(test)]
    pub fn test(color: bool, width: usize, resource: Option<&str>) -> Self {
        Self {
            color,
            width,
            resource: resource.map(|s| s.to_string()),
        }
    }
}

/// Format a JSON API response for output.
///
/// - `Json`: Pretty-printed JSON (ignores color/resource)
/// - `Text`: Human-readable formatted output with optional color
///
/// If `fields` is provided, only those fields are included in the output
/// (client-side field masking for context window discipline).
pub fn format_output(
    value: &Value,
    format: OutputFormat,
    fields: Option<&str>,
    ctx: Option<&TextContext>,
) -> String {
    let filtered = match fields {
        Some(field_list) => filter_fields(value, field_list),
        None => value.clone(),
    };

    match format {
        OutputFormat::Json => {
            serde_json::to_string_pretty(&filtered).unwrap_or_else(|_| filtered.to_string())
        }
        OutputFormat::Text => {
            let default_ctx = TextContext::new(false, None);
            let ctx = ctx.unwrap_or(&default_ctx);
            format_text(&filtered, fields, ctx)
        }
    }
}

/// Filter response to only include specified fields.
///
/// Works on the `data` field of the response. Handles both single objects
/// and arrays of objects.
///
/// Supports dot-notation field paths for nested structures:
///   --fields "id,title"                  → top-level fields
///   --fields "document.id,document.title,context"  → nested fields (e.g. search results)
fn filter_fields(value: &Value, field_list: &str) -> Value {
    let fields: Vec<&str> = field_list.split(',').map(|f| f.trim()).collect();

    let mut result = value.clone();

    if let Some(data) = result.get_mut("data") {
        match data {
            Value::Object(obj) => {
                let filtered = filter_object_fields(obj, &fields);
                *data = Value::Object(filtered);
            }
            Value::Array(arr) => {
                let filtered: Vec<Value> = arr
                    .iter()
                    .map(|item| {
                        if let Value::Object(obj) = item {
                            Value::Object(filter_object_fields(obj, &fields))
                        } else {
                            item.clone()
                        }
                    })
                    .collect();
                *data = Value::Array(filtered);
            }
            _ => {}
        }
    }

    result
}

/// Filter an object to only include specified fields.
///
/// Supports dot-notation paths: "document.id" extracts `obj["document"]["id"]`
/// and places it at "document.id" in the output (flattened key).
pub fn filter_object_fields(
    obj: &serde_json::Map<String, Value>,
    fields: &[&str],
) -> serde_json::Map<String, Value> {
    let mut result = serde_json::Map::new();

    for &field in fields {
        if let Some((parent, child)) = field.split_once('.') {
            // Dot-notation: "document.id" → obj["document"]["id"]
            if let Some(parent_val) = obj.get(parent) {
                // Recursively resolve deeper paths (e.g., "a.b.c")
                if let Some(resolved) = resolve_dot_path(parent_val, child) {
                    // Place under parent key as nested object
                    let parent_obj = result
                        .entry(parent.to_string())
                        .or_insert_with(|| Value::Object(serde_json::Map::new()));
                    if let Value::Object(m) = parent_obj {
                        m.insert(child.to_string(), resolved);
                    }
                }
            }
        } else {
            // Simple field: "id" → obj["id"]
            if let Some(val) = obj.get(field) {
                result.insert(field.to_string(), val.clone());
            }
        }
    }

    result
}

/// Resolve a dot-separated path into a JSON value.
/// "b.c" on {"b": {"c": 1}} → Some(1)
fn resolve_dot_path(value: &Value, path: &str) -> Option<Value> {
    if let Some((head, tail)) = path.split_once('.') {
        value.get(head).and_then(|v| resolve_dot_path(v, tail))
    } else {
        value.get(path).cloned()
    }
}

// ---------------------------------------------------------------------------
// Text formatting: dispatch based on response shape
// ---------------------------------------------------------------------------

/// Format a JSON response as human-readable text.
fn format_text(value: &Value, fields: Option<&str>, ctx: &TextContext) -> String {
    // Error response
    if value.get("ok") == Some(&Value::Bool(false)) {
        return format_error(value, ctx);
    }

    match value.get("data") {
        Some(Value::Array(items)) if !items.is_empty() => {
            if is_search_result(items) {
                format_search_results(items, ctx)
            } else {
                format_list(items, fields, ctx)
            }
        }
        Some(Value::Array(_)) => "No results.".to_string(),
        Some(Value::Object(_)) => format_detail(&value["data"], fields, ctx),
        _ => {
            // Status-only response (e.g., ok: true with no data)
            if value.get("ok") == Some(&Value::Bool(true)) {
                format_status_ok(value, ctx)
            } else {
                serde_json::to_string_pretty(value).unwrap_or_default()
            }
        }
    }
}

/// Check if array items look like search results (have "document" + "context" keys).
fn is_search_result(items: &[Value]) -> bool {
    items
        .first()
        .map(|item| item.get("document").is_some() && item.get("context").is_some())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Error formatting
// ---------------------------------------------------------------------------

fn format_error(value: &Value, ctx: &TextContext) -> String {
    let error_text = value["message"]
        .as_str()
        .or_else(|| value["error"].as_str())
        .unwrap_or("Unknown error");
    let status = value
        .get("status")
        .or_else(|| value.get("code"))
        .and_then(|v| v.as_u64());

    let symbol = if ctx.color { "\u{2717}" } else { "ERROR" };
    let prefix = color::red_bold(symbol, ctx.color);

    match status {
        Some(code) => format!("{prefix} {error_text} ({code})"),
        None => format!("{prefix} {error_text}"),
    }
}

// ---------------------------------------------------------------------------
// Status OK formatting
// ---------------------------------------------------------------------------

fn format_status_ok(value: &Value, ctx: &TextContext) -> String {
    let symbol = if ctx.color { "\u{2713}" } else { "OK" };
    let prefix = color::green(symbol, ctx.color);

    // Try to extract a meaningful message from the response
    if let Some(action) = value.get("action").and_then(|a| a.as_str()) {
        let resource = value.get("resource").and_then(|r| r.as_str()).unwrap_or("");
        format!("{prefix} {action} {resource} successful")
    } else {
        format!("{prefix} Success")
    }
}

// ---------------------------------------------------------------------------
// List formatting (table)
// ---------------------------------------------------------------------------

/// Smart default fields per resource type for text output.
fn default_fields_for_resource(resource: Option<&str>) -> &'static [&'static str] {
    match resource {
        Some("collections") => &["id", "name"],
        Some("documents") => &["id", "title", "updatedAt"],
        Some("users") => &["id", "name", "email"],
        Some("groups") => &["id", "name", "memberCount"],
        Some("comments") => &["id", "createdAt"],
        Some("stars") => &["id", "documentId", "createdAt"],
        Some("views") => &["id", "documentId", "count"],
        _ => &["id", "name", "title"],
    }
}

fn format_list(items: &[Value], fields: Option<&str>, ctx: &TextContext) -> String {
    // Determine which fields to show
    let field_names: Vec<&str> = match fields {
        Some(field_list) => field_list.split(',').map(|f| f.trim()).collect(),
        None => default_fields_for_resource(ctx.resource.as_deref()).to_vec(),
    };

    // Build headers (uppercase field names)
    let headers: Vec<String> = field_names.iter().map(|f| f.to_uppercase()).collect();

    let mut table = Table::new(headers);

    for item in items {
        let values: Vec<String> = field_names
            .iter()
            .map(|&field| {
                if let Some((parent, child)) = field.split_once('.') {
                    // Dot-notation: "document.title"
                    item.get(parent)
                        .and_then(|p| p.get(child))
                        .map(value_to_display)
                        .unwrap_or_default()
                } else {
                    item.get(field).map(value_to_display).unwrap_or_default()
                }
            })
            .collect();
        table.add_row(values);
    }

    table.render(ctx.width, ctx.color)
}

// ---------------------------------------------------------------------------
// Detail formatting (single object)
// ---------------------------------------------------------------------------

fn format_detail(data: &Value, fields: Option<&str>, ctx: &TextContext) -> String {
    let field_names: Vec<&str> = match fields {
        Some(field_list) => field_list.split(',').map(|f| f.trim()).collect(),
        None => {
            // Collect keys that have non-null values, in a sensible order
            let priority_keys = [
                "title",
                "name",
                "id",
                "url",
                "email",
                "createdAt",
                "updatedAt",
                "text",
            ];
            let mut keys: Vec<&str> = Vec::new();
            for &key in &priority_keys {
                if data.get(key).is_some_and(|v| !v.is_null()) {
                    keys.push(key);
                }
            }
            keys
        }
    };

    if field_names.is_empty() {
        return serde_json::to_string_pretty(data).unwrap_or_default();
    }

    // Find the longest label for alignment
    let max_label_len = field_names
        .iter()
        .map(|f| display_label(f).len())
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();

    for &field in &field_names {
        let value = if let Some((parent, child)) = field.split_once('.') {
            data.get(parent).and_then(|p| p.get(child))
        } else {
            data.get(field)
        };

        if let Some(val) = value {
            let label = display_label(field);
            let padded_label = format!("{:<width$}", label, width = max_label_len);
            let colored_label = color::blue(&padded_label, ctx.color);

            let display_val = format_detail_value(field, val, ctx);
            lines.push(format!("{}  {}", colored_label, display_val));
        }
    }

    lines.join("\n")
}

/// Convert a field name to a display label (capitalize first letter).
fn display_label(field: &str) -> String {
    // Handle dot-notation: "document.title" → "Title"
    let name = field.rsplit('.').next().unwrap_or(field);

    // Convert camelCase to Title Case: "updatedAt" → "Updated At"
    let mut label = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            label.push(' ');
        }
        if i == 0 {
            label.extend(c.to_uppercase());
        } else {
            label.push(c);
        }
    }
    label
}

/// Format a value for the detail view.
fn format_detail_value(field: &str, val: &Value, ctx: &TextContext) -> String {
    match val {
        Value::String(s) => {
            // Special formatting for known field types
            if field == "url" || field.ends_with("Url") {
                color::cyan_underline(s, ctx.color)
            } else if field.ends_with("At") {
                color::yellow(s, ctx.color)
            } else if field == "id" || field.ends_with("Id") {
                color::dim(s, ctx.color)
            } else if field == "text" {
                // Truncate long text content
                let line_count = s.lines().count();
                if line_count > 5 {
                    let preview: String = s.lines().take(3).collect::<Vec<_>>().join("\n");
                    format!(
                        "{}\n{}",
                        preview,
                        color::dim(
                            &format!("({line_count} lines, use --output json for full content)"),
                            ctx.color,
                        )
                    )
                } else {
                    s.clone()
                }
            } else {
                s.clone()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => color::dim("null", ctx.color),
        _ => serde_json::to_string(val).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Search result formatting
// ---------------------------------------------------------------------------

fn format_search_results(items: &[Value], ctx: &TextContext) -> String {
    let mut lines = Vec::new();

    for (i, item) in items.iter().enumerate() {
        let doc = &item["document"];
        let title = doc["title"].as_str().unwrap_or("(untitled)");
        let doc_id = doc["id"].as_str().unwrap_or("");
        let context = item["context"].as_str().unwrap_or("").trim();

        // Numbered title
        let num = format!("{}.", i + 1);
        let title_line = format!(
            "  {} {}",
            color::dim(&num, ctx.color),
            color::bold(title, ctx.color),
        );
        lines.push(title_line);

        // Context snippet
        if !context.is_empty() {
            let preview: String = context
                .chars()
                .take(120)
                .collect::<String>()
                .replace('\n', " ");
            lines.push(format!("     {}", preview));
        }

        // ID
        lines.push(format!("     {}", color::dim(doc_id, ctx.color)));
        lines.push(String::new());
    }

    // Summary
    lines.push(format!("{} result(s)", items.len()));

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Convert a JSON value to a display string for table cells.
fn value_to_display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        _ => serde_json::to_string(v).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_ctx() -> TextContext {
        TextContext::test(false, 80, None)
    }

    fn test_ctx_with_resource(resource: &str) -> TextContext {
        TextContext::test(false, 80, Some(resource))
    }

    fn test_ctx_color() -> TextContext {
        TextContext::test(true, 80, None)
    }

    // --- JSON format tests (unchanged behavior) ---

    #[test]
    fn json_format_pretty_prints() {
        let val = json!({"ok": true, "data": {"id": "abc"}});
        let out = format_output(&val, OutputFormat::Json, None, None);
        assert!(out.contains("\"ok\": true"));
        assert!(out.contains("\"id\": \"abc\""));
    }

    // --- Error formatting ---

    #[test]
    fn text_format_shows_error() {
        let val = json!({"ok": false, "error": "Not Found", "status": 404});
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        assert!(out.contains("Not Found"));
        assert!(out.contains("404"));
    }

    #[test]
    fn text_format_prefers_message_over_error() {
        let val = json!({"ok": false, "error": "validation_error", "message": "id: Invalid", "status": 400});
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        assert!(out.contains("id: Invalid"));
        assert!(out.contains("400"));
    }

    #[test]
    fn text_format_falls_back_to_error_when_no_message() {
        let val = json!({"ok": false, "error": "rate_limit_exceeded", "status": 429});
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        assert!(out.contains("rate_limit_exceeded"));
        assert!(out.contains("429"));
    }

    #[test]
    fn error_with_color_uses_cross_mark() {
        let val = json!({"ok": false, "error": "Not Found", "status": 404});
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx_color()));
        assert!(out.contains("\u{2717}"));
    }

    #[test]
    fn error_without_color_uses_error_text() {
        let val = json!({"ok": false, "error": "Not Found", "status": 404});
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        assert!(out.contains("ERROR"));
    }

    // --- Status OK formatting ---

    #[test]
    fn status_ok_with_color_uses_check_mark() {
        let val = json!({"ok": true});
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx_color()));
        assert!(out.contains("\u{2713}"));
    }

    #[test]
    fn status_ok_without_color_uses_ok_text() {
        let val = json!({"ok": true});
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        assert!(out.contains("OK"));
    }

    // --- List formatting ---

    #[test]
    fn text_format_lists_as_table() {
        let val = json!({
            "ok": true,
            "data": [
                {"id": "abc", "title": "First Document", "updatedAt": "2026-03-12"},
                {"id": "def", "title": "Second Document", "updatedAt": "2026-03-11"}
            ]
        });
        let ctx = test_ctx_with_resource("documents");
        let out = format_output(&val, OutputFormat::Text, None, Some(&ctx));
        // Should have a header row
        assert!(out.contains("ID"));
        assert!(out.contains("TITLE"));
        // Should have data rows
        assert!(out.contains("abc"));
        assert!(out.contains("First Document"));
        assert!(out.contains("def"));
        assert!(out.contains("Second Document"));
    }

    #[test]
    fn list_uses_smart_defaults_for_collections() {
        let val = json!({
            "ok": true,
            "data": [
                {"id": "abc", "name": "Plans", "description": "not shown"},
            ]
        });
        let ctx = test_ctx_with_resource("collections");
        let out = format_output(&val, OutputFormat::Text, None, Some(&ctx));
        assert!(out.contains("ID"));
        assert!(out.contains("NAME"));
        assert!(out.contains("Plans"));
        // description is not in default fields for collections
        assert!(!out.contains("not shown"));
    }

    #[test]
    fn list_respects_explicit_fields() {
        let val = json!({
            "ok": true,
            "data": [
                {"id": "abc", "name": "Plans", "description": "shown now"},
            ]
        });
        let ctx = test_ctx_with_resource("collections");
        let out = format_output(
            &val,
            OutputFormat::Text,
            Some("name,description"),
            Some(&ctx),
        );
        assert!(out.contains("NAME"));
        assert!(out.contains("DESCRIPTION"));
        assert!(out.contains("shown now"));
        // id is not in explicit fields
        assert!(!out.contains("abc"));
    }

    #[test]
    fn empty_list_shows_no_results() {
        let val = json!({"ok": true, "data": []});
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        assert_eq!(out, "No results.");
    }

    // --- Detail formatting ---

    #[test]
    fn detail_shows_labeled_fields() {
        let val = json!({
            "ok": true,
            "data": {
                "id": "abc-123",
                "title": "Test Doc",
                "url": "/doc/test",
                "updatedAt": "2026-03-12T09:00:00Z"
            }
        });
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        assert!(out.contains("Title"));
        assert!(out.contains("Test Doc"));
        assert!(out.contains("Id"));
        assert!(out.contains("abc-123"));
        assert!(out.contains("Url"));
        assert!(out.contains("Updated At"));
    }

    #[test]
    fn detail_truncates_long_text() {
        let long_text = (0..20)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let val = json!({
            "ok": true,
            "data": {
                "title": "Long Doc",
                "text": long_text,
            }
        });
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        assert!(out.contains("20 lines, use --output json for full content"));
    }

    // --- Search result formatting ---

    #[test]
    fn search_results_format_with_context() {
        let val = json!({
            "ok": true,
            "data": [
                {
                    "context": "...the onboarding process...",
                    "ranking": 0.95,
                    "document": {
                        "id": "abc-123",
                        "title": "Onboarding Guide",
                        "text": "full content"
                    }
                }
            ]
        });
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        assert!(out.contains("1."));
        assert!(out.contains("Onboarding Guide"));
        assert!(out.contains("onboarding process"));
        assert!(out.contains("abc-123"));
        assert!(out.contains("1 result(s)"));
    }

    #[test]
    fn search_results_detected_by_shape() {
        // Even without explicit resource context, search results are detected by shape
        let val = json!({
            "ok": true,
            "data": [
                {"context": "snippet", "document": {"id": "x", "title": "T"}}
            ]
        });
        let out = format_output(&val, OutputFormat::Text, None, Some(&test_ctx()));
        // Should format as search results, not a table
        assert!(out.contains("1."));
        assert!(out.contains("1 result(s)"));
    }

    // --- Field masking (unchanged behavior) ---

    #[test]
    fn field_masking_filters_object() {
        let val = json!({
            "ok": true,
            "data": {
                "id": "abc",
                "title": "Test",
                "text": "long content...",
                "url": "https://example.com"
            }
        });
        let out = format_output(&val, OutputFormat::Json, Some("id,title"), None);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert!(parsed["data"]["id"].is_string());
        assert!(parsed["data"]["title"].is_string());
        assert!(parsed["data"]["text"].is_null());
    }

    #[test]
    fn field_masking_filters_array() {
        let val = json!({
            "ok": true,
            "data": [
                {"id": "1", "title": "A", "text": "content"},
                {"id": "2", "title": "B", "text": "content"}
            ]
        });
        let out = format_output(&val, OutputFormat::Json, Some("id,title"), None);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let arr = parsed["data"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr[0]["text"].is_null());
    }

    #[test]
    fn field_masking_dot_notation_nested() {
        let val = json!({
            "ok": true,
            "data": [
                {
                    "context": "some snippet",
                    "ranking": 0.99,
                    "document": {
                        "id": "abc",
                        "title": "Test Doc",
                        "text": "long content..."
                    }
                }
            ]
        });
        let out = format_output(
            &val,
            OutputFormat::Json,
            Some("document.id,document.title,context"),
            None,
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let arr = parsed["data"].as_array().unwrap();
        assert_eq!(arr[0]["document"]["id"], "abc");
        assert_eq!(arr[0]["document"]["title"], "Test Doc");
        assert_eq!(arr[0]["context"], "some snippet");
        assert!(arr[0]["document"]["text"].is_null());
        assert!(arr[0]["ranking"].is_null());
    }

    #[test]
    fn field_masking_mixed_flat_and_dot() {
        let val = json!({
            "ok": true,
            "data": {
                "name": "test",
                "nested": {"a": 1, "b": 2},
                "extra": "removed"
            }
        });
        let out = format_output(&val, OutputFormat::Json, Some("name,nested.a"), None);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["data"]["name"], "test");
        assert_eq!(parsed["data"]["nested"]["a"], 1);
        assert!(parsed["data"]["nested"]["b"].is_null());
        assert!(parsed["data"]["extra"].is_null());
    }

    // --- display_label tests ---

    #[test]
    fn display_label_simple() {
        assert_eq!(display_label("id"), "Id");
        assert_eq!(display_label("name"), "Name");
    }

    #[test]
    fn display_label_camel_case() {
        assert_eq!(display_label("updatedAt"), "Updated At");
        assert_eq!(display_label("collectionId"), "Collection Id");
    }

    #[test]
    fn display_label_dot_notation() {
        assert_eq!(display_label("document.title"), "Title");
    }

    // --- is_search_result detection ---

    #[test]
    fn detects_search_results() {
        let items = vec![json!({"document": {"id": "x"}, "context": "snippet"})];
        assert!(is_search_result(&items));
    }

    #[test]
    fn non_search_results_detected() {
        let items = vec![json!({"id": "x", "title": "test"})];
        assert!(!is_search_result(&items));
    }
}
