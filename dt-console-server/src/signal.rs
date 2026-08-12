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
    if pid == 0 {
        // kill(0, sig) signals the whole process group — never what we mean.
        return Err(format!("refusing to send {} to pid 0", sig.name()));
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, sig.as_libc()) };
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

/// Check whether `pid` is alive, via the null signal.
#[cfg(unix)]
pub fn is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // Signal 0 performs the permission and existence checks without sending.
    // EPERM means the process exists but is not ours — still "alive".
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Check whether `pid` is alive (non-Unix fallback).
#[cfg(not(unix))]
pub fn is_alive(_pid: u32) -> bool {
    false
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
        assert!(err.contains("pid 0"), "unexpected error: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn test_send_to_dead_pid_reports_process_gone() {
        // A pid this high is not in use; ESRCH must map to success.
        let outcome = send(4_194_303, EngineSignal::Term).expect("ESRCH must not be an error");
        assert_eq!(outcome, SignalOutcome::ProcessGone);
    }

    #[cfg(unix)]
    #[test]
    fn test_send_signal_zero_equivalent_to_self_is_delivered() {
        let me = std::process::id();
        // SIGUSR1/2 would actually be delivered; use is_alive (signal 0) for self.
        assert!(is_alive(me), "the test process must be alive");
    }

    #[cfg(unix)]
    #[test]
    fn test_is_alive_false_for_dead_pid() {
        assert!(!is_alive(4_194_303));
        assert!(!is_alive(0));
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
