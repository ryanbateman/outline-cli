//! MCP protocol conformance tests.
//!
//! These tests spawn the `outline mcp` process and communicate via
//! JSON-RPC over stdin/stdout to verify MCP protocol compliance.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Helper: spawn the MCP server process with given args.
/// Returns (child, stdin, stdout_reader).
fn spawn_mcp(
    args: &[&str],
) -> (
    std::process::Child,
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_outline"));
    cmd.arg("mcp");
    for arg in args {
        cmd.arg(arg);
    }
    // Need a valid token for the server to start (even though we won't make real API calls in most tests)
    cmd.env(
        "OUTLINE_API_TOKEN",
        "ol_api_testtoken1234567890abcdefghijklmnop",
    );
    cmd.env("OUTLINE_API_URL", "http://localhost:99999/api");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("Failed to spawn outline mcp");
    let stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = BufReader::new(child.stdout.take().expect("Failed to get stdout"));

    (child, stdin, stdout)
}

/// Send a JSON-RPC message and read the response.
fn send_and_receive(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut BufReader<std::process::ChildStdout>,
    msg: &Value,
) -> Value {
    let msg_str = serde_json::to_string(msg).unwrap();
    writeln!(stdin, "{}", msg_str).expect("Failed to write to stdin");
    stdin.flush().expect("Failed to flush stdin");

    let mut line = String::new();
    stdout
        .read_line(&mut line)
        .expect("Failed to read from stdout");
    serde_json::from_str(line.trim()).expect("Failed to parse JSON-RPC response")
}

/// Send a notification (no response expected).
fn send_notification(stdin: &mut std::process::ChildStdin, msg: &Value) {
    let msg_str = serde_json::to_string(msg).unwrap();
    writeln!(stdin, "{}", msg_str).expect("Failed to write notification");
    stdin.flush().expect("Failed to flush stdin");
    // Small delay to let the server process it
    std::thread::sleep(Duration::from_millis(100));
}

/// Perform the MCP initialize handshake. Returns the initialize result.
fn do_handshake(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut BufReader<std::process::ChildStdout>,
) -> Value {
    let init_msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1.0"}
        }
    });
    let result = send_and_receive(stdin, stdout, &init_msg);

    // Send initialized notification
    send_notification(
        stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    );

    result
}

#[test]
fn mcp_initialize_returns_valid_response() {
    let (mut child, mut stdin, mut stdout) = spawn_mcp(&[]);

    let resp = do_handshake(&mut stdin, &mut stdout);

    // Verify JSON-RPC structure
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);

    // Verify MCP initialize result
    let result = &resp["result"];
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["serverInfo"]["name"], "outline-mcp");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["instructions"].is_string());

    drop(stdin);
    let _ = child.wait();
}

#[test]
fn mcp_tools_list_returns_all_methods() {
    let (mut child, mut stdin, mut stdout) = spawn_mcp(&[]);
    do_handshake(&mut stdin, &mut stdout);

    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    );

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);

    let tools = resp["result"]["tools"]
        .as_array()
        .expect("tools should be array");
    assert!(!tools.is_empty(), "Should have tools");

    // Verify tools include documents and collections
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(
        names.contains(&"documents.create"),
        "Should have documents.create"
    );
    assert!(
        names.contains(&"documents.list"),
        "Should have documents.list"
    );
    assert!(
        names.contains(&"documents.search"),
        "Should have documents.search"
    );
    assert!(
        names.contains(&"collections.list"),
        "Should have collections.list"
    );

    drop(stdin);
    let _ = child.wait();
}

#[test]
fn mcp_tools_list_respects_expose_filter() {
    let (mut child, mut stdin, mut stdout) = spawn_mcp(&["--expose", "collections"]);
    do_handshake(&mut stdin, &mut stdout);

    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    );

    let tools = resp["result"]["tools"]
        .as_array()
        .expect("tools should be array");

    // All tools should be collections.* only
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            name.starts_with("collections."),
            "Expected only collections tools with --expose collections, got: {name}"
        );
    }

    drop(stdin);
    let _ = child.wait();
}

#[test]
fn mcp_tools_have_valid_input_schemas() {
    let (mut child, mut stdin, mut stdout) = spawn_mcp(&["--expose", "documents"]);
    do_handshake(&mut stdin, &mut stdout);

    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    );

    let tools = resp["result"]["tools"]
        .as_array()
        .expect("tools should be array");

    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let schema = &tool["inputSchema"];

        // Every tool must have an inputSchema object
        assert!(schema.is_object(), "Tool {name} missing inputSchema");

        // Description should be present
        assert!(
            tool["description"].is_string(),
            "Tool {name} missing description"
        );
    }

    drop(stdin);
    let _ = child.wait();
}

#[test]
fn mcp_tool_call_validates_input() {
    let (mut child, mut stdin, mut stdout) = spawn_mcp(&["--expose", "documents"]);
    do_handshake(&mut stdin, &mut stdout);

    // Call with a hallucinated ID containing query params
    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "documents.info",
                "arguments": {"id": "abc?key=val"}
            }
        }),
    );

    let result = &resp["result"];
    assert_eq!(result["isError"], true, "Should be an error response");

    // Content should contain validation error
    let content_text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        content_text.contains("input_validation_error"),
        "Should report input validation error, got: {content_text}"
    );

    drop(stdin);
    let _ = child.wait();
}

#[test]
fn mcp_tool_call_unknown_method_returns_error() {
    let (mut child, mut stdin, mut stdout) = spawn_mcp(&[]);
    do_handshake(&mut stdin, &mut stdout);

    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "nonexistent.method",
                "arguments": {}
            }
        }),
    );

    let result = &resp["result"];
    assert_eq!(result["isError"], true);

    let content_text = result["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("Unknown method"));

    drop(stdin);
    let _ = child.wait();
}

#[test]
fn mcp_tool_call_invalid_name_format() {
    let (mut child, mut stdin, mut stdout) = spawn_mcp(&[]);
    do_handshake(&mut stdin, &mut stdout);

    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "no-dot-separator",
                "arguments": {}
            }
        }),
    );

    let result = &resp["result"];
    assert_eq!(result["isError"], true);

    let content_text = result["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("resource.action"));

    drop(stdin);
    let _ = child.wait();
}

#[test]
fn mcp_tool_call_filtered_resource_returns_error() {
    let (mut child, mut stdin, mut stdout) = spawn_mcp(&["--expose", "collections"]);
    do_handshake(&mut stdin, &mut stdout);

    // Try to call a documents method when only collections are exposed
    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "documents.list",
                "arguments": {}
            }
        }),
    );

    let result = &resp["result"];
    assert_eq!(result["isError"], true);

    let content_text = result["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("not exposed"));

    drop(stdin);
    let _ = child.wait();
}
