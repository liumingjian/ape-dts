//! Direct signal delivery to engine subprocesses.
//!
//! Every signal the orchestrator sends to a `dt-main` child goes through this
//! module. It replaces the previous `std::process::Command::new("kill")` calls,
//! which fork a shell-less child, ignore its non-zero exit status (`.output()`
//! only fails when the *spawn* fails), and therefore report "signal sent" even
//! when the signal was never delivered — leaving the console to mark a Run
//! `stopped` while the engine keeps running.
//!
//! `libc::kill` reports the real outcome via errno:
//!
//! - `ESRCH` — no such process. The process is already gone, which is exactly
//!   the state the caller wanted, so this is [`SignalOutcome::ProcessGone`],
//!   a success.
//! - `EPERM` — the process exists but belongs to someone else. The signal was
//!   *not* delivered; this must surface as an error.
//! - anything else (`EINVAL`, …) — a programming error; also an error.

/// A signal the orchestrator sends to an engine subprocess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineSignal {
    /// Graceful shutdown.
    Term,
    /// Forced kill.
    Kill,
    /// Pause (engine-defined, SIGUSR1).
    Pause,
    /// Resume (engine-defined, SIGUSR2).
    Resume,
}

impl EngineSignal {
    /// The signal's POSIX name, for log and error messages.
    pub fn name(self) -> &'static str {
        match self {
            EngineSignal::Term => "SIGTERM",
            EngineSignal::Kill => "SIGKILL",
            EngineSignal::Pause => "SIGUSR1",
            EngineSignal::Resume => "SIGUSR2",
        }
    }

    #[cfg(unix)]
    fn as_libc(self) -> libc::c_int {
        match self {
            EngineSignal::Term => libc::SIGTERM,
            EngineSignal::Kill => libc::SIGKILL,
            EngineSignal::Pause => libc::SIGUSR1,
            EngineSignal::Resume => libc::SIGUSR2,
        }
    }
}

/// What happened when a signal was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalOutcome {
    /// The signal reached a live process.
    Delivered,
    /// The process no longer exists (`ESRCH`). Treated as success: the
    /// caller's intent (process not running / already reaped) already holds.
    ProcessGone,
}

/// Send `sig` to `pid`.
///
/// Returns `Ok(Delivered)` on success, `Ok(ProcessGone)` if the process has
/// already exited, and `Err(reason)` when the signal could not be delivered to
/// a process that may still be running (`EPERM` and friends).
#[cfg(unix)]
pub fn send(pid: u32, sig: EngineSignal) -> Result<SignalOutcome, String> {
    let pid =
        checked_pid(pid).map_err(|reason| format!("refusing to send {}: {reason}", sig.name()))?;
    let rc = unsafe { libc::kill(pid, sig.as_libc()) };
    if rc == 0 {
        return Ok(SignalOutcome::Delivered);
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ESRCH) => Ok(SignalOutcome::ProcessGone),
        Some(libc::EPERM) => Err(format!(
            "not permitted to send {} to pid {pid}: {err}",
            sig.name()
        )),
        _ => Err(format!("failed to send {} to pid {pid}: {err}", sig.name())),
    }
}

/// Send a signal (non-Unix fallback — the orchestrator only supports Unix).
#[cfg(not(unix))]
pub fn send(pid: u32, sig: EngineSignal) -> Result<SignalOutcome, String> {
    Err(format!(
        "{} to pid {pid} is not supported on this platform",
        sig.name()
    ))
}

/// Narrow a stored pid to one that is safe to pass to `kill`.
///
/// `kill` reads negative pids as "a process group" and `0` as "my whole
/// process group", so a pid that arrives corrupted — a negative or oversized
/// `run.pid` column, a bad migration — must never reach the syscall. The old
/// shell-out was accidentally safe here (`kill -s TERM 4294967295` just
/// errored); the syscall is not.
#[cfg(unix)]
fn checked_pid(pid: u32) -> Result<libc::pid_t, String> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(format!("{pid} is not a valid process id"));
    }
    Ok(pid as libc::pid_t)
}

/// Check whether `pid` is a live process **we can signal**.
///
/// Uses the null signal, which performs the existence and permission checks
/// without sending anything. A process we are not permitted to signal
/// (`EPERM`) counts as not alive: it cannot be ours, so it is either a
/// recycled pid or a foreign process, and treating it as a live engine would
/// leave a Run that can neither finish nor be stopped.
#[cfg(unix)]
pub fn is_alive(pid: u32) -> bool {
    match checked_pid(pid) {
        Ok(pid) => unsafe { libc::kill(pid, 0) == 0 },
        Err(_) => false,
    }
}

/// Check whether `pid` is alive (non-Unix fallback).
#[cfg(not(unix))]
pub fn is_alive(_pid: u32) -> bool {
    false
}

/// A pid that is genuinely dead: spawn a child, kill it, reap it.
///
/// Test-only. Picking a large constant instead would be a slow-burning flake —
/// the kernel is free to hand that number to somebody else's process, and the
/// test would then signal a stranger.
#[cfg(all(test, unix))]
pub(crate) fn reaped_pid() -> u32 {
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    let _ = child.kill();
    let _ = child.wait();
    pid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_names() {
        assert_eq!(EngineSignal::Term.name(), "SIGTERM");
        assert_eq!(EngineSignal::Kill.name(), "SIGKILL");
        assert_eq!(EngineSignal::Pause.name(), "SIGUSR1");
        assert_eq!(EngineSignal::Resume.name(), "SIGUSR2");
    }

    #[test]
    fn test_send_to_pid_zero_is_rejected() {
        // pid 0 means "the whole process group" — never the intent here.
        let err = send(0, EngineSignal::Term).unwrap_err();
        assert!(err.contains("not a valid process id"), "unexpected: {err}");
    }

    #[test]
    fn test_send_to_out_of_range_pid_is_rejected() {
        // `as i32` would make this negative — a whole process group.
        let err = send(u32::MAX, EngineSignal::Term).unwrap_err();
        assert!(err.contains("not a valid process id"), "unexpected: {err}");
        assert!(!is_alive(u32::MAX));
    }

    #[cfg(unix)]
    #[test]
    fn test_send_to_dead_pid_reports_process_gone() {
        let outcome = send(reaped_pid(), EngineSignal::Term).expect("ESRCH must not be an error");
        assert_eq!(outcome, SignalOutcome::ProcessGone);
    }

    #[cfg(unix)]
    #[test]
    fn test_is_alive_true_for_self() {
        assert!(is_alive(std::process::id()), "the test process is alive");
    }

    #[cfg(unix)]
    #[test]
    fn test_is_alive_false_for_dead_pid() {
        assert!(!is_alive(reaped_pid()));
        assert!(!is_alive(0));
    }

    #[cfg(unix)]
    #[test]
    fn test_is_alive_false_for_a_process_we_cannot_signal() {
        // pid 1 is init: it exists, but an unprivileged process gets EPERM.
        // "Alive but not ours" must read as not alive — a recycled pid must
        // never keep a Run pinned in `running` forever.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        assert!(!is_alive(1));
    }

    #[cfg(unix)]
    #[test]
    fn test_send_term_to_live_child_is_delivered() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        assert_eq!(
            send(pid, EngineSignal::Term).unwrap(),
            SignalOutcome::Delivered
        );
        let _ = child.wait();
        // After reaping, the pid is gone.
        assert_eq!(
            send(pid, EngineSignal::Term).unwrap(),
            SignalOutcome::ProcessGone
        );
    }
}
