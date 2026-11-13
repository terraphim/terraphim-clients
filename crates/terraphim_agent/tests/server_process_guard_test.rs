//! Regression tests for the real server process lifetime guard.

#![cfg(target_os = "linux")]

use serial_test::serial;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

mod support;
use support::binary::{server_binary_path, skip_without_server};
use support::server_process::ServerProcessGuard;

/// Start a server for the calling test, or `None` when this machine has no
/// server binary.
///
/// CI installs terraphim-ai v1.21.3 before running this suite; a local
/// checkout usually has no server at all, in which case the test skips with
/// the shared notice instead of failing (Refs #466).
fn spawn_server(test: &str) -> Option<ServerProcessGuard> {
    if skip_without_server(test) {
        return None;
    }
    let binary = server_binary_path()
        .expect("server_binary_path must resolve when skip_without_server returned false");
    let port = portpicker::pick_unused_port().expect("find unused server port");
    let mut command = Command::new(binary);
    command
        .args([
            "--config",
            "terraphim_server/default/terraphim_engineer_config.json",
        ])
        .env("TERRAPHIM_SERVER_HOSTNAME", format!("127.0.0.1:{port}"));
    Some(ServerProcessGuard::spawn(&mut command).expect("start terraphim_server through guard"))
}

fn assert_process_exists(pid: u32) {
    assert!(
        Path::new(&format!("/proc/{pid}")).exists(),
        "guarded server process {pid} exited before its lifetime could be tested"
    );
}

fn assert_process_reaped(pid: u32) {
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "guarded server process {pid} still exists after guard drop"
    );
}

#[test]
#[serial]
fn drop_stops_and_reaps_server() {
    let Some(server) = spawn_server("drop_stops_and_reaps_server") else {
        return;
    };
    let pid = server.id();
    assert_process_exists(pid);

    drop(server);

    assert_process_reaped(pid);
}

#[test]
#[serial]
fn leader_exit_is_observed_without_reaping_before_drop() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "exit 23"]);
    let mut server = ServerProcessGuard::spawn(&mut command).expect("start short-lived leader");
    let pid = server.id();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = server.try_wait().expect("poll leader without reaping") {
            break status;
        }
        assert!(Instant::now() < deadline, "leader did not exit");
        thread::sleep(Duration::from_millis(10));
    };

    assert_eq!(status.code(), Some(23));
    let proc_status = std::fs::read_to_string(format!("/proc/{pid}/status"))
        .expect("leader remains present until guard drop");
    assert!(
        proc_status.lines().any(|line| line == "State:\tZ (zombie)"),
        "leader was reaped before group shutdown:\n{proc_status}"
    );

    drop(server);
    assert_process_reaped(pid);
}

#[test]
#[serial]
fn panic_unwinding_stops_and_reaps_server() {
    if skip_without_server("panic_unwinding_stops_and_reaps_server") {
        return;
    }
    let mut pid = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let server = spawn_server("panic_unwinding_stops_and_reaps_server")
            .expect("server binary was resolved before this test started");
        pid = Some(server.id());
        assert_process_exists(server.id());
        panic!("exercise guard drop during panic unwinding");
    }));

    assert!(result.is_err(), "test closure must panic");
    assert_process_reaped(pid.expect("server started before panic"));
}
