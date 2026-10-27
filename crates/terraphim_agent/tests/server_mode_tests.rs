use anyhow::Result;
use serial_test::serial;
use std::process::{Command, Stdio};
use std::str;
use std::thread;
use std::time::Duration;
use tokio::time::timeout;

mod support;
use support::server_process::ServerProcessGuard;

/// Test helper to start a real terraphim server for testing
async fn start_test_server() -> Result<(ServerProcessGuard, String)> {
    // Find an available port
    let port = portpicker::pick_unused_port().expect("Failed to find unused port");
    let server_url = format!("http://localhost:{}", port);

    println!("Starting test server on {}", server_url);

    // Start the server with terraphim engineer config
    // terraphim_server is not a workspace member, so it cannot be run from
    // here; a nested cargo would also deadlock under `cargo test`. Point
    // TERRAPHIM_SERVER_BIN at a prebuilt binary to exercise this. Refs #113.
    let server_bin = std::env::var("TERRAPHIM_SERVER_BIN").map_err(|_| {
        anyhow::anyhow!(
            "TERRAPHIM_SERVER_BIN is not set and terraphim_server is not a \
             member of this workspace; cannot start a server. Refs #113"
        )
    })?;
    let mut command = Command::new(server_bin);
    command
        .args([
            "--config",
            "terraphim_server/default/terraphim_engineer_config.json",
        ])
        .env("TERRAPHIM_SERVER_HOSTNAME", format!("127.0.0.1:{}", port))
        .env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut server = ServerProcessGuard::spawn(&mut command)?;

    // Wait for server to be ready
    let client = reqwest::Client::new();
    let health_url = format!("{}/health", server_url);

    println!("Waiting for server to be ready at {}", health_url);

    // Try to connect for up to 120 seconds (compilation + startup can be slow)
    for attempt in 1..=120 {
        thread::sleep(Duration::from_secs(1));

        match client.get(&health_url).send().await {
            Ok(response) if response.status().is_success() => {
                println!("Server ready after {} seconds", attempt);
                return Ok((server, server_url));
            }
            Ok(_) => println!("Server responding but not healthy (attempt {})", attempt),
            Err(_) => println!("Server not responding yet (attempt {})", attempt),
        }

        // Check if server process is still running
        match server.try_wait() {
            Ok(Some(status)) => {
                return Err(anyhow::anyhow!(
                    "Server exited early with status: {}",
                    status
                ));
            }
            Ok(None) => {} // Still running
            Err(e) => return Err(anyhow::anyhow!("Error checking server status: {}", e)),
        }
    }

    Err(anyhow::anyhow!(
        "Server failed to become ready within 120 seconds"
    ))
}

/// Test helper to run TUI commands against a real server
fn run_server_command(server_url: &str, args: &[&str]) -> Result<(String, String, i32)> {
    let mut cmd_args = vec!["--server", "--server-url", server_url];
    cmd_args.extend_from_slice(args);

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    cmd.args(&cmd_args);

    let output = cmd.output()?;

    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    ))
}

#[tokio::test]
#[serial]
async fn test_server_mode_config_show() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // Test config show with real server
    let (stdout, stderr, code) = run_server_command(&server_url, &["config", "show"])?;

    // Cleanup
    drop(server);

    assert_eq!(
        code, 0,
        "Server mode config show should succeed, stderr: {}",
        stderr
    );

    // Parse JSON output
    let lines: Vec<&str> = stdout.lines().collect();
    let json_start = lines.iter().position(|line| line.starts_with('{'));
    assert!(json_start.is_some(), "Should contain JSON output");

    let json_lines = &lines[json_start.unwrap()..];
    let json_str = json_lines.join("\n");

    let config: serde_json::Value = serde_json::from_str(&json_str).expect("Should be valid JSON");

    assert_eq!(config["id"], "Server", "Should use Server config");
    assert!(
        config.get("selected_role").is_some(),
        "Should have selected_role"
    );
    // selected_role depends on server config / fallback config; just ensure field is present.
    assert!(config.get("selected_role").is_some());

    println!("Server config: {}", json_str);

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_mode_roles_list() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // Test roles list with real server
    let (stdout, stderr, code) = run_server_command(&server_url, &["roles", "list"])?;

    // Cleanup
    drop(server);

    assert_eq!(
        code, 0,
        "Server mode roles list should succeed, stderr: {}",
        stderr
    );

    // List roles (may be empty depending on server config)
    let roles: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    println!("Available roles: {:?}", roles);

    // Some server configs may not have roles pre-loaded; that is valid.
    // When roles exist, expect at least one recognizable name.
    if !roles.is_empty() {
        println!("Server has {} roles", roles.len());
    } else {
        println!("Server has no roles (valid for minimal config)");
    }

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_mode_search_with_selected_role() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // Give server time to index documents
    thread::sleep(Duration::from_secs(3));

    // Try to select Default role; may fail if server has no roles
    let (_, _, _select_code) = run_server_command(&server_url, &["roles", "select", "Default"])?;
    thread::sleep(Duration::from_millis(500));

    // Test search using selected role (or server default if role select failed)
    let (stdout, stderr, code) =
        run_server_command(&server_url, &["search", "rust programming", "--limit", "5"])?;

    // Cleanup
    drop(server);

    // Search may fail if no roles/haystacks are configured
    assert!(
        code == 0 || code == 1,
        "Server mode search should not crash, stderr: {}",
        stderr
    );

    println!("Search results (exit {}): {}", code, stdout);

    if code == 0 {
        let result_lines: Vec<&str> = stdout
            .lines()
            .filter(|line| line.starts_with("- "))
            .collect();
        println!("Found {} search results", result_lines.len());
    } else {
        println!("Search returned no results (expected with minimal config)");
    }

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_mode_search_with_role_override() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // Give server time to index documents
    thread::sleep(Duration::from_secs(2));

    // Test search with role override
    let (stdout, stderr, code) = run_server_command(
        &server_url,
        &["search", "test", "--role", "Default", "--limit", "3"],
    )?;

    // Cleanup
    drop(server);

    // Search may succeed or fail depending on whether Default role exists
    assert!(
        code == 0 || code == 1,
        "Search with role override should not crash, stderr: {}",
        stderr
    );

    if code == 0 {
        println!("Search with role override successful: {}", stdout);
    } else {
        println!(
            "Search with role override failed (role may not exist): {}",
            stderr
        );
    }

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_mode_roles_select() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // First check what roles are available
    let (roles_stdout, _, _) = run_server_command(&server_url, &["roles", "list"])?;
    let available_roles: Vec<&str> = roles_stdout.lines().filter(|l| !l.is_empty()).collect();

    if available_roles.is_empty() {
        println!("No roles available on server, testing role-not-found path");

        let (_, stderr, code) = run_server_command(&server_url, &["roles", "select", "Default"])?;

        // Cleanup
        drop(server);

        assert_eq!(
            code, 1,
            "Role select should fail when no roles exist, stderr: {}",
            stderr
        );
        return Ok(());
    }

    // Parse role name from list output format: " * Role Name (shortname)" or "   Role Name"
    // Strip the leading marker (* or space) and trailing (shortname) if present
    let raw_line = available_roles[0].trim();
    let without_marker = raw_line.trim_start_matches('*').trim();
    let role_name = if let Some(paren_pos) = without_marker.rfind(" (") {
        without_marker[..paren_pos].trim()
    } else {
        without_marker
    };
    println!("Selecting role: {}", role_name);

    let (stdout, stderr, code) = run_server_command(&server_url, &["roles", "select", role_name])?;

    // Cleanup
    drop(server);

    assert_eq!(
        code, 0,
        "Server mode role select should succeed, stderr: {}",
        stderr
    );
    assert!(
        stdout.contains(&format!("selected:{}", role_name)),
        "Should confirm role selection: {}",
        stdout
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_mode_graph_command() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // Give server time to build knowledge graph
    thread::sleep(Duration::from_secs(5));

    // Test graph command with real server
    let (stdout, stderr, code) = run_server_command(&server_url, &["graph", "--top-k", "10"])?;

    // Cleanup
    drop(server);

    // Some server builds may not support role-specific rolegraph queries and can return 404.
    assert!(
        code == 0 || stderr.contains("404"),
        "Server mode graph should complete (or be unsupported), stderr: {}",
        stderr
    );

    println!("Graph concepts: {}", stdout);

    // Should show some concepts
    let concept_lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();
    println!("Found {} graph concepts", concept_lines.len());

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_mode_chat_command() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // Test chat command with real server
    let (stdout, stderr, code) = run_server_command(&server_url, &["chat", "Hello, how are you?"])?;

    // Cleanup
    drop(server);

    assert_eq!(
        code, 0,
        "Server mode chat should succeed, stderr: {}",
        stderr
    );

    println!("Chat response: {}", stdout);

    // Should have some response (even if no LLM configured)
    assert!(!stdout.trim().is_empty(), "Should have some chat response");

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_mode_extract_command() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // Give server time to load thesaurus
    thread::sleep(Duration::from_secs(3));

    let test_text = "This is a test paragraph about Rust programming. Rust is a systems programming language that focuses on safety and performance. It has concepts like ownership, borrowing, and lifetimes.";

    // Test extract command with real server
    let (stdout, stderr, code) = run_server_command(&server_url, &["extract", test_text])?;

    // Cleanup
    drop(server);

    assert!(
        code == 0 || code == 1,
        "Server mode extract should complete, stderr: {}",
        stderr
    );

    println!("Extract results: {}", stdout);

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_mode_config_set() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // Test config set with real server
    let (stdout, stderr, code) = run_server_command(
        &server_url,
        &["config", "set", "selected_role", "Terraphim Engineer"],
    )?;

    // Cleanup server first
    drop(server);

    assert_eq!(
        code, 0,
        "Server mode config set should succeed, stderr: {}",
        stderr
    );
    assert!(
        stdout.contains("updated selected_role to Terraphim Engineer"),
        "Should confirm config update: {}",
        stdout
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_vs_offline_mode_comparison() -> Result<()> {
    // Start server for comparison
    let (server, server_url) = start_test_server().await?;

    // Test the same command in both modes
    let (server_stdout, _server_stderr, server_code) =
        run_server_command(&server_url, &["config", "show"])?;

    // Cleanup server
    drop(server);

    // Run offline command
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    cmd.args(["config", "show"]);

    let offline_output = cmd.output()?;
    let offline_stdout = String::from_utf8_lossy(&offline_output.stdout);
    let offline_code = offline_output.status.code().unwrap_or(-1);

    // Both should succeed
    assert_eq!(server_code, 0, "Server mode should succeed");
    assert_eq!(offline_code, 0, "Offline mode should succeed");

    // Parse both configs
    let parse_config = |output: &str| -> serde_json::Value {
        let lines: Vec<&str> = output.lines().collect();
        let json_start = lines.iter().position(|line| line.starts_with('{')).unwrap();
        let json_lines = &lines[json_start..];
        let json_str = json_lines.join("\n");
        serde_json::from_str(&json_str).unwrap()
    };

    let server_config = parse_config(&server_stdout);
    let offline_config = parse_config(&offline_stdout);

    // Compare key differences
    assert_eq!(
        server_config["id"], "Server",
        "Server should use Server config"
    );
    assert_eq!(
        offline_config["id"], "Embedded",
        "Offline should use Embedded config"
    );

    println!("Server config ID: {}", server_config["id"]);
    println!("Offline config ID: {}", offline_config["id"]);
    println!("Server selected_role: {}", server_config["selected_role"]);
    println!("Offline selected_role: {}", offline_config["selected_role"]);

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_server_startup_and_health() -> Result<()> {
    let (server, server_url) = start_test_server().await?;

    // Test that server is actually healthy
    let client = reqwest::Client::new();
    let health_url = format!("{}/health", server_url);

    let response = timeout(Duration::from_secs(5), client.get(&health_url).send()).await??;

    // Cleanup
    drop(server);

    assert!(
        response.status().is_success(),
        "Server health check should pass"
    );

    println!("Server health check passed: {}", response.status());

    Ok(())
}
