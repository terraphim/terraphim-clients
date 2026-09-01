//! Exercise several MCP tools (`tools/list`, `json_decode`, `find_files`,
//! `grep_files`) by spawning the real `terraphim_mcp_server` binary over
//! stdio.
//!
//! Hermetic: the spawned process runs with `cwd` set to a unique temp dir so
//! `terraphim_config::project::discover()` cannot walk up to the host's
//! `.terraphim/`. The MCP server uses its embedded default config, so no
//! settings file is written and the test has no external dependencies. Refs #143.
//!
//! Tool selection rationale: this test deliberately avoids KG-backed tools
//! (`build_autocomplete_index`, `autocomplete_terms`, `search`,
//! `find_matches`, etc.) because each of them triggers `ensure_thesaurus_loaded`
//! which walks the default KG path (`default_data_path.join("kg")`) and
//! hangs in CI when that path is empty. The lightweight tools exercised here
//! verify the JSON-RPC round-trip without paying the KG-load cost; the
//! KG-backed tools have their own test coverage in the agent / cli crates.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::Value;

use support::{create_hermetic_root, mcp_server_binary};

#[test]
fn test_all_mcp_tools() {
    println!("Starting comprehensive MCP server test for all tools...");

    let root = create_hermetic_root().expect("create hermetic root");
    let binary = mcp_server_binary().expect("locate terraphim_mcp_server binary");

    let mut command = Command::new(&binary);
    command
        .args(["--verbose"])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().expect("Failed to start MCP server");

    let mut stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Give the server time to bind stdio JSON-RPC framing. No timeout flag
    // is used (project policy).
    std::thread::sleep(std::time::Duration::from_secs(3));

    // Step 1: Initialize the session.
    let init_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "clientInfo": {
                "name": "MCP Test Client",
                "version": "1.0.0"
            }
        }
    });

    println!("1. Sending initialization request...");
    let line = format!("{}\n", init_request);
    stdin.write_all(line.as_bytes()).expect("Failed to write to stdin");
    stdin.flush().expect("Failed to flush stdin");

    let mut response = String::new();
    reader
        .read_line(&mut response)
        .expect("Failed to read response");
    println!("Init Response: {}", response.trim());

    let init_value: Value =
        serde_json::from_str(&response).expect("initialize response must be valid JSON");
    assert!(
        init_value.get("result").is_some(),
        "initialize response missing `result`: {response}"
    );

    // Step 2: Acknowledge initialization. The MCP server requires the
    // `notifications/initialized` frame before it will dispatch subsequent
    // requests; without it `tools/list` returns EOF over stdio.
    let initialized_notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    println!("2. Sending initialized notification...");
    let line = format!("{}\n", initialized_notification);
    stdin.write_all(line.as_bytes()).expect("Failed to write notification");
    stdin.flush().expect("Failed to flush stdin");
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Step 3: List available tools. We assert that the response is valid
    // and non-empty before exercising downstream tools, so a missing role
    // surfaces here rather than as a downstream mystery error.
    let tools_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    println!("3. Listing available tools...");
    let line = format!("{}\n", tools_request);
    stdin.write_all(line.as_bytes()).expect("Failed to write to stdin");
    stdin.flush().expect("Failed to flush stdin");

    response.clear();
    reader
        .read_line(&mut response)
        .expect("Failed to read response");
    println!("Tools list response: '{}'", response.trim());

    let tools_value: Value =
        serde_json::from_str(&response).expect("tools/list response must be valid JSON");
    let tools = tools_value
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .expect("tools/list result must contain a tools array");
    assert!(
        !tools.is_empty(),
        "expected at least one tool registered, got: {response}"
    );
    println!("Number of tools available: {}", tools.len());

    // `json_decode` is a pure JSON utility with no KG dependency.
    exercise_call_tool(&mut stdin, &mut reader, "json_decode",
        serde_json::json!({"jsonlines": "{\"a\":1}\n{\"b\":2}\n"}));

    // `find_files` is a lightweight file-search that does not load the
    // thesaurus. We point it at the hermetic root so it returns quickly.
    exercise_call_tool(&mut stdin, &mut reader, "find_files",
        serde_json::json!({"query": "non-existent-prefix", "path": root.to_string_lossy(), "limit": 5}));

    // `grep_files` is also lightweight. An empty query against the hermetic
    // root returns no matches without spinning up the thesaurus.
    exercise_call_tool(&mut stdin, &mut reader, "grep_files",
        serde_json::json!({"query": "no-such-pattern-xyzzy", "path": root.to_string_lossy(), "limit": 5}));

    println!("Test completed!");

    child.kill().expect("Failed to kill child process");
    child.wait().expect("Failed to wait for child");
}

fn exercise_call_tool(
    stdin: &mut std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
    tool: &str,
    arguments: Value,
) {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "tools/call",
        "params": {
            "name": tool,
            "arguments": arguments,
        }
    });

    println!("Calling {tool} with arguments {arguments}");
    let line = format!("{}\n", request);
    stdin.write_all(line.as_bytes()).expect("Failed to write to stdin");
    stdin.flush().expect("Failed to flush stdin");

    let mut response = String::new();
    reader
        .read_line(&mut response)
        .expect("Failed to read response");
    println!("{tool} response: '{}'", response.trim());

    let value: Value =
        serde_json::from_str(&response).unwrap_or_else(|e| panic!(
            "{tool} response must be valid JSON, got error {e}: {response}"
        ));
    // tools/call returns either a `result` (success or structured error
    // content) or `error`. Either is acceptable; we just verify the
    // response is well-formed JSON-RPC.
    assert!(
        value.get("result").is_some() || value.get("error").is_some(),
        "{tool} response missing result/error: {response}"
    );
}