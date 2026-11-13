//! Spawn the real `terraphim_mcp_server` binary over stdio and assert that the
//! `initialize` response declares the capabilities the server implements.
//! Clients that gate on capabilities (for example Zed's context-server lane)
//! show no tools when `capabilities` is empty. Hermetic like
//! `test_tools_list.rs`: the server runs in a temporary working directory.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::thread;

use serde_json::Value;

use support::{create_hermetic_root, mcp_server_binary};

fn initialize_result() -> Value {
    let root = create_hermetic_root().expect("create hermetic root");
    let binary = mcp_server_binary().expect("locate terraphim_mcp_server binary");

    let mut child = Command::new(&binary)
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start MCP server");

    // Drain stderr so the pipe buffer never fills and kills the server.
    let stderr = child.stderr.take().expect("stderr");
    let drain = thread::spawn(move || BufReader::new(stderr).lines().map_while(Result::ok).count());

    let mut stdin = child.stdin.take().expect("stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "capabilities-test", "version": "1.0.0"}
        }
    });
    writeln!(stdin, "{request}").expect("write initialize");
    stdin.flush().expect("flush");

    // The server answers once it is ready; reading blocks until it does.
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read initialize response");

    drop(stdin);
    child.kill().ok();
    child.wait().ok();
    let _ = drain.join();

    let response: Value = serde_json::from_str(&line).expect("initialize response is JSON");
    response
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("initialize response has no result: {line}"))
}

#[test]
fn initialize_declares_exactly_the_implemented_capabilities() {
    let result = initialize_result();
    // Exactly tools and resources, with no sub-capabilities (listChanged,
    // subscribe) and nothing for prompts, logging, completions or
    // experimental, none of which the ServerHandler implements.
    assert_eq!(
        result["capabilities"],
        serde_json::json!({"tools": {}, "resources": {}}),
        "initialize capabilities must match the implemented ServerHandler methods"
    );
}
