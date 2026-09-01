//! Smoke test: spawn the real `terraphim_mcp_server` binary over stdio and
//! drive the MCP protocol to list the registered tools.
//!
//! Hermetic: the spawned process runs with `cwd` set to a unique temp dir so
//! `terraphim_config::project::discover()` cannot walk up to the host's
//! `.terraphim/` (which would otherwise make the server load an unrelated
//! project config and risk crashing on missing role references). No
//! `TERRAPHIM_SETTINGS_PATH` is set — the MCP server binary does not read
//! that variable; only `terraphim_settings::DeviceSettings` does, and that
//! path is not exercised here. Stderr is drained on a background thread so
//! the OS pipe buffer cannot fill and kill the server prematurely. Refs #143.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;

use support::{create_hermetic_root, mcp_server_binary};

#[test]
fn test_tools_list_only() {
    println!("Starting MCP server test for tools list...");

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

    // Drain stderr on a background thread so its pipe buffer never fills
    // and SIGPIPEs the server. The captured log is exposed for post-mortem.
    let stderr_log = Arc::new(Mutex::new(String::new()));
    let stderr_log_thread = {
        let stderr_log = Arc::clone(&stderr_log);
        let stderr = child.stderr.take().expect("get stderr");
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                stderr_log.lock().expect("stderr log mutex").push_str(&line);
                stderr_log.lock().expect("stderr log mutex").push('\n');
            }
        })
    };

    let mut stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Give the server time to bind stdio JSON-RPC framing before sending
    // any requests. No timeout flag is used (project policy).
    thread::sleep(std::time::Duration::from_secs(3));

    // Step 1: Send initialization request.
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
    match stdin.write_all(line.as_bytes()) {
        Ok(()) => {}
        Err(e) => {
            child.kill().ok();
            child.wait().ok();
            let _ = stderr_log_thread.join();
            let log = stderr_log.lock().expect("stderr log mutex").clone();
            panic!(
                "broken pipe writing initialize request ({e}); server stderr:\n{log}"
            );
        }
    }
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

    // Step 2: Send initialized notification (required by MCP protocol).
    let initialized_notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    println!("2. Sending initialized notification...");
    let line = format!("{}\n", initialized_notification);
    stdin.write_all(line.as_bytes()).expect("Failed to write notification");
    stdin.flush().expect("Failed to flush stdin");
    thread::sleep(std::time::Duration::from_millis(100));

    // Step 3: List available tools.
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
        "expected at least one tool, got: {response}"
    );
    println!("Number of tools available: {}", tools.len());

    child.kill().expect("Failed to kill child process");
    child.wait().expect("Failed to wait for child");
    let _ = stderr_log_thread.join();
}
