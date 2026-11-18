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
}

impl ServerProcessGuard {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        command.process_group(0);
        command.spawn().map(|child| Self { child })
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Poll the leader without reaping it.
    ///
    /// `WNOWAIT` keeps an exited leader as a zombie, reserving its PID and the
    /// process-group identity until `Drop` has signalled the group.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        // SAFETY: `info` points to writable storage and waitid initialises it
        // when reporting a child state. WNOWAIT deliberately leaves the child
        // waitable for the final `Child::wait` in Drop.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: waitid returned successfully, so the siginfo storage is
        // initialised. si_pid == 0 means WNOHANG found no state change.
        let info = unsafe { info.assume_init() };
        if unsafe { info.si_pid() } == 0 {
            return Ok(None);
        }

        Ok(Some(exit_status_from_siginfo(&info)))
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
}

fn exit_status_from_siginfo(info: &libc::siginfo_t) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;

    let code = info.si_code;
    let status = unsafe { info.si_status() };
    let raw = match code {
        libc::CLD_EXITED => status << 8,
        libc::CLD_KILLED => status,
        libc::CLD_DUMPED => status | 0x80,
        _ => status << 8,
    };
    ExitStatus::from_raw(raw)
}

impl Drop for ServerProcessGuard {
    fn drop(&mut self) {
        self.signal_group(SIGTERM);
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while self.group_exists() && Instant::now() < deadline {
            thread::sleep(POLL_INTERVAL);
        }

        if self.group_exists() {
            self.signal_group(SIGKILL);
        }
        let _ = self.child.wait();
    }
}
