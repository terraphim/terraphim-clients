#![allow(dead_code)]

use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

const SIGNAL_EXISTS: i32 = 0;
const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;
const ESRCH: i32 = 3;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

/// Owns a test server and guarantees that its whole process group is stopped.
///
/// Every real `terraphim_server` started by the agent integration tests must go
/// through this type. A separate process group prevents descendants from
/// surviving cancellation, while `Drop` also covers test panic unwinding.
pub struct ServerProcessGuard {
    child: Child,
    child_reaped: bool,
}

impl ServerProcessGuard {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        command.process_group(0);
        command.spawn().map(|child| Self {
            child,
            child_reaped: false,
        })
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.child_reaped = true;
        }
        Ok(status)
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    fn process_group(&self) -> i32 {
        -(self.child.id() as i32)
    }

    fn signal_group(&self, signal: i32) {
        // SAFETY: `kill` is called with a process-group id created by
        // `process_group(0)` and a standard POSIX signal number.
        unsafe {
            kill(self.process_group(), signal);
        }
    }

    fn group_exists(&self) -> bool {
        // SAFETY: signal zero only tests whether the process group exists.
        let result = unsafe { kill(self.process_group(), SIGNAL_EXISTS) };
        result == 0 || io::Error::last_os_error().raw_os_error() != Some(ESRCH)
    }

    fn reap_if_exited(&mut self) {
        if self.child_reaped {
            return;
        }
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            self.child_reaped = true;
        }
    }
}

impl Drop for ServerProcessGuard {
    fn drop(&mut self) {
        self.signal_group(SIGTERM);
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while self.group_exists() && Instant::now() < deadline {
            self.reap_if_exited();
            thread::sleep(POLL_INTERVAL);
        }

        if self.group_exists() {
            self.signal_group(SIGKILL);
        }
        if !self.child_reaped {
            let _ = self.child.wait();
            self.child_reaped = true;
        }
    }
}
