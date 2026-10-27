//! Regression tests for the real server process lifetime guard.

#![cfg(target_os = "linux")]

use serial_test::serial;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

mod support;
use support::server_process::ServerProcessGuard;

fn server_binary() -> PathBuf {
    std::env::var_os("TERRAPHIM_SERVER_BIN")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .expect(
            "TERRAPHIM_SERVER_BIN must point to a real terraphim_server binary; \
             CI installs terraphim-ai v1.21.3 before running this suite",
        )
}

fn spawn_server() -> ServerProcessGuard {
    let port = portpicker::pick_unused_port().expect("find unused server port");
    let mut command = Command::new(server_binary());
    command
        .args([
            "--config",
            "terraphim_server/default/terraphim_engineer_config.json",
        ])
        .env("TERRAPHIM_SERVER_HOSTNAME", format!("127.0.0.1:{port}"));
    ServerProcessGuard::spawn(&mut command).expect("start terraphim_server through guard")
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
    let server = spawn_server();
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
    let mut pid = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let server = spawn_server();
        pid = Some(server.id());
        assert_process_exists(server.id());
        panic!("exercise guard drop during panic unwinding");
    }));

    assert!(result.is_err(), "test closure must panic");
    assert_process_reaped(pid.expect("server started before panic"));
}
