//! Executor trait and LocalExecutor implementation.
//!
//! The `Executor` trait defines the interface for spawning, killing, and
//! observing engine subprocesses. `LocalExecutor` fork-execs the `dt-main`
//! binary (or a path overridden via `APE_DTS_BINARY_PATH`) on the orchestrator
//! host.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Default engine binary path (relative to workspace root).
const DEFAULT_ENGINE_BINARY: &str = "target/release/dt-main";

/// Environment variable to override the engine binary path.
const ENGINE_BINARY_ENV: &str = "APE_DTS_BINARY_PATH";

/// Default grace window (seconds) before escalating SIGTERM to SIGKILL.
const DEFAULT_GRACE_WINDOW_SECS: u64 = 10;

/// Environment variable to override the grace window.
const GRACE_WINDOW_ENV: &str = "CONSOLE_STOP_GRACE_SECS";

/// Base directory for per-Run working directories.
const DEFAULT_RUN_DATA_DIR: &str = "./data/runs";

/// Environment variable to override the run data directory.
const RUN_DATA_DIR_ENV: &str = "CONSOLE_RUN_DATA_DIR";

/// Handle to a spawned Run, tracking the child process.
#[derive(Debug)]
pub struct RunHandle {
    /// The Run's unique ID.
    pub run_id: String,
    /// The child process ID.
    pub pid: u32,
    /// Path to the Run's working directory.
    pub run_dir: PathBuf,
    /// The managed child process.
    pub child: Arc<Mutex<Option<tokio::process::Child>>>,
    /// Whether this handle was re-attached after orchestrator restart.
    ///
    /// When `true`, `child` is `None` and process liveness is checked
    /// via `kill(pid, 0)` instead of `child.try_wait()`.
    pub reattached: bool,
}

impl Clone for RunHandle {
    fn clone(&self) -> Self {
        Self {
            run_id: self.run_id.clone(),
            pid: self.pid,
            run_dir: self.run_dir.clone(),
            child: self.child.clone(),
            reattached: self.reattached,
        }
    }
}

/// Slot in the active-runs registry.
///
/// `Starting` means the start handler has claimed the slot but the engine
/// subprocess has not yet been spawned. `Active` means the engine is
/// running (or at least the child PID is known).
#[derive(Debug)]
pub enum RunSlot {
    /// Slot claimed; engine subprocess not yet spawned.
    Starting,
    /// Engine subprocess is running (or was running).
    Active(RunHandle),
}

impl Clone for RunSlot {
    fn clone(&self) -> Self {
        match self {
            RunSlot::Starting => RunSlot::Starting,
            RunSlot::Active(h) => RunSlot::Active(h.clone()),
        }
    }
}

impl RunSlot {
    /// Return a reference to the inner `RunHandle`, if `Active`.
    pub fn as_handle(&self) -> Option<&RunHandle> {
        match self {
            RunSlot::Starting => None,
            RunSlot::Active(h) => Some(h),
        }
    }

    /// Return the inner `RunHandle`, if `Active`.
    pub fn into_handle(self) -> Option<RunHandle> {
        match self {
            RunSlot::Starting => None,
            RunSlot::Active(h) => Some(h),
        }
    }
}

/// Child process exit status.
#[derive(Debug, Clone)]
pub enum ExitStatus {
    /// Process exited normally with an exit code.
    Exited { code: i32 },
    /// Process was terminated by a signal.
    Signaled { signal: i32 },
}

/// The status of a Run's child process.
#[derive(Debug, Clone)]
pub enum ChildStatus {
    /// Child is still running.
    Running,
    /// Child has exited.
    Exited(ExitStatus),
}

/// Result of a kill operation.
#[derive(Debug, Clone)]
pub struct KillResult {
    /// The method used to stop the process.
    pub stop_method: String,
    /// The exit status of the process.
    pub exit_status: ExitStatus,
}

/// A chunk of log data read from a Run's log file.
#[derive(Debug, Clone)]
pub struct LogChunk {
    /// The log file name (e.g. "default", "position").
    pub file: String,
    /// The line content (without trailing newline).
    pub line: String,
}

/// Resolve the engine binary path from the environment or default.
pub fn engine_binary_path() -> String {
    std::env::var(ENGINE_BINARY_ENV).unwrap_or_else(|_| DEFAULT_ENGINE_BINARY.to_string())
}

/// Resolve the grace window from the environment or default.
pub fn grace_window_secs() -> u64 {
    std::env::var(GRACE_WINDOW_ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_GRACE_WINDOW_SECS)
}

/// Resolve the base directory for per-Run working directories.
pub fn run_data_dir() -> String {
    std::env::var(RUN_DATA_DIR_ENV).unwrap_or_else(|_| DEFAULT_RUN_DATA_DIR.to_string())
}

/// LocalExecutor: spawns engine subprocesses on the local host.
pub struct LocalExecutor;

impl LocalExecutor {
    /// Spawn a new engine subprocess for the given Run.
    ///
    /// - Creates the per-Run working directory at `<run_data_dir>/<run_id>/`.
    /// - Writes the rendered INI to `<run_dir>/task_config.ini`.
    /// - Creates an empty `<run_dir>/logs/` directory.
    /// - Fork-execs the engine binary with the INI path as the sole argument.
    /// - Returns a `RunHandle` tracking the child process.
    pub async fn spawn(
        run_id: &str,
        ini_content: &str,
        binary_override: Option<&str>,
    ) -> Result<RunHandle, String> {
        Self::spawn_with_env(run_id, ini_content, binary_override, &[]).await
    }

    pub async fn spawn_with_env(
        run_id: &str,
        ini_content: &str,
        binary_override: Option<&str>,
        extra_env: &[(String, String)],
    ) -> Result<RunHandle, String> {
        let base_dir = run_data_dir();
        let run_dir = PathBuf::from(&base_dir).join(run_id);

        // Create run directory structure.
        let logs_dir = run_dir.join("logs");
        std::fs::create_dir_all(&logs_dir)
            .map_err(|e| format!("failed to create run directory {:?}: {e}", run_dir))?;

        // The child process is spawned with cwd=run_dir, so any relative
        // path in the INI resolves against run_dir. The log4rs config file
        // lives at the orchestrator's cwd (typically the repo root), so a
        // relative `log4rs_file` would not be found by the child. Rewrite it
        // to an absolute path so log4rs initialises and the engine writes its
        // per-run finished/position/default logs into `<run_dir>/logs/`
        // instead of falling silently back to a shared `<repo>/logs/`.
        let prepared_ini = absolutize_log4rs_path(ini_content);

        // Write INI file.
        let ini_path = run_dir.join("task_config.ini");
        std::fs::write(&ini_path, &prepared_ini)
            .map_err(|e| format!("failed to write INI file {:?}: {e}", ini_path))?;

        // Canonicalize the INI path to absolute so the child process can find it
        // even though current_dir is changed to run_dir.
        let ini_path_abs = std::fs::canonicalize(&ini_path)
            .map_err(|e| format!("failed to canonicalize INI path {:?}: {e}", ini_path))?;

        // Resolve engine binary path.
        let engine_binary = binary_override
            .map(|s| s.to_string())
            .unwrap_or_else(engine_binary_path);

        // If the binary is a path (contains a separator), canonicalize it to
        // absolute before spawn. Otherwise current_dir(&run_dir) would make
        // execve resolve relative paths against the child's cwd, not the
        // orchestrator's. Bare command names are left for PATH lookup.
        let engine_program: PathBuf = if engine_binary.contains(std::path::MAIN_SEPARATOR) {
            std::fs::canonicalize(&engine_binary)
                .map_err(|e| format!("failed to resolve engine binary '{engine_binary}': {e}"))?
        } else {
            PathBuf::from(&engine_binary)
        };

        // Spawn the child process.
        let mut command = tokio::process::Command::new(&engine_program);
        command
            .arg(ini_path_abs.to_string_lossy().as_ref())
            .current_dir(&run_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let child = command
            .spawn()
            .map_err(|e| format!("failed to spawn engine binary {:?}: {e}", engine_program))?;

        let pid = child.id().unwrap_or(0);

        Ok(RunHandle {
            run_id: run_id.to_string(),
            pid,
            run_dir,
            child: Arc::new(Mutex::new(Some(child))),
            reattached: false,
        })
    }

    /// Check the status of a spawned child process.
    ///
    /// Returns `ChildStatus::Running` if the child is still alive,
    /// or `ChildStatus::Exited` with the exit code/signal if it has terminated.
    ///
    /// For re-attached processes (where `child` is `None` and `reattached` is
    /// `true`), process liveness is checked via `kill(pid, 0)` on Unix.
    pub async fn status(handle: &RunHandle) -> ChildStatus {
        // Re-attached processes: no Child object, check PID liveness directly.
        if handle.reattached {
            return pid_status(handle.pid);
        }

        let mut guard = handle.child.lock().await;
        if let Some(ref mut child) = *guard {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Child has exited. Remove it from the handle.
                    let exit = if let Some(code) = status.code() {
                        ExitStatus::Exited { code }
                    } else {
                        // On Unix, signal termination returns None for code().
                        // We approximate the signal as 9 (SIGKILL) or 15 (SIGTERM)
                        // based on common patterns; the exact signal is hard to get
                        // from std::process::ExitStatus on all platforms.
                        ExitStatus::Signaled {
                            signal: if status.code().is_none() { 9 } else { 0 },
                        }
                    };
                    // Remove the child from the handle since it's done.
                    *guard = None;
                    ChildStatus::Exited(exit)
                }
                Ok(None) => ChildStatus::Running,
                Err(_) => {
                    // Error checking status — treat as still running.
                    ChildStatus::Running
                }
            }
        } else {
            // Child was already consumed (exited previously).
            ChildStatus::Exited(ExitStatus::Exited { code: -1 })
        }
    }

    /// Kill a spawned child process with graceful shutdown.
    ///
    /// 1. Sends SIGTERM to the child.
    /// 2. Waits up to `grace_window_secs()` for the child to exit.
    /// 3. If the child doesn't exit within the grace window, sends SIGKILL.
    ///
    /// For re-attached processes, sends SIGTERM/KILL directly via PID
    /// and polls for process exit via `kill(pid, 0)`.
    ///
    /// Returns a `KillResult` describing how the process was stopped.
    pub async fn kill(handle: &RunHandle) -> Result<KillResult, String> {
        let grace = grace_window_secs();
        Self::kill_with_grace(handle, grace).await
    }

    /// Kill with a specific grace window (for testing).
    pub async fn kill_with_grace(
        handle: &RunHandle,
        grace_secs: u64,
    ) -> Result<KillResult, String> {
        // Re-attached processes: no Child object, signal directly via PID.
        if handle.reattached {
            return kill_reattached(handle.pid, grace_secs).await;
        }

        let mut guard = handle.child.lock().await;
        let child = guard
            .as_mut()
            .ok_or_else(|| "child process already consumed".to_string())?;

        // Send SIGTERM first.
        let pid = child.id().unwrap_or(0);
        if pid > 0 {
            send_sigterm(pid)?;
        }

        // Wait for the child to exit within the grace window.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(grace_secs);
        let mut stop_method = "sigterm".to_string();

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let exit = exit_status_from_process(&status);
                    *guard = None;
                    return Ok(KillResult {
                        stop_method,
                        exit_status: exit,
                    });
                }
                Ok(None) => {
                    if tokio::time::Instant::now() >= deadline {
                        // Grace window expired; send SIGKILL.
                        if pid > 0 {
                            send_sigkill(pid)?;
                        }
                        stop_method = "sigkill".to_string();

                        // Wait for SIGKILL to take effect (should be immediate).
                        let kill_deadline =
                            tokio::time::Instant::now() + tokio::time::Duration::from_secs(2);
                        loop {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    let exit = exit_status_from_process(&status);
                                    *guard = None;
                                    return Ok(KillResult {
                                        stop_method,
                                        exit_status: exit,
                                    });
                                }
                                Ok(None) => {
                                    if tokio::time::Instant::now() >= kill_deadline {
                                        // Forced kill didn't work within 2s.
                                        // Try to reap anyway.
                                        *guard = None;
                                        return Ok(KillResult {
                                            stop_method,
                                            exit_status: ExitStatus::Signaled { signal: 9 },
                                        });
                                    }
                                    tokio::time::sleep(tokio::time::Duration::from_millis(100))
                                        .await;
                                }
                                Err(_) => {
                                    *guard = None;
                                    return Ok(KillResult {
                                        stop_method,
                                        exit_status: ExitStatus::Signaled { signal: 9 },
                                    });
                                }
                            }
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(e) => {
                    *guard = None;
                    return Err(format!("error checking child status: {e}"));
                }
            }
        }
    }

    /// Re-attach to an already-running engine subprocess after orchestrator restart.
    ///
    /// Creates a `RunHandle` that tracks the process by PID without a `Child`
    /// object (since the process was spawned by a previous orchestrator session).
    /// The `reattached` flag is set to `true` so that `status()` and `kill()`
    /// use PID-based checking instead of `child.try_wait()`.
    pub fn reattach(run_id: &str, pid: u32, run_dir: PathBuf) -> RunHandle {
        RunHandle {
            run_id: run_id.to_string(),
            pid,
            run_dir,
            child: Arc::new(Mutex::new(None)),
            reattached: true,
        }
    }

    /// Read the position from a Run's position.log file.
    ///
    /// Returns `None` if the file doesn't exist or is empty.
    /// Returns `Some(Value)` with the parsed position data if available.
    pub fn read_position(run_dir: &Path) -> Option<serde_json::Value> {
        let position_path = run_dir.join("logs").join("position.log");
        if !position_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&position_path).ok()?;
        if content.trim().is_empty() {
            return None;
        }

        // The position.log may contain multiple lines; read the last non-empty line.
        let last_line = content.lines().last()?.trim();
        if last_line.is_empty() {
            return None;
        }

        // Try to parse as JSON first; if that fails, return as a plain string.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(last_line) {
            Some(v)
        } else {
            // Parse key=value format from the engine's position log.
            Some(parse_position_kv(last_line))
        }
    }
}

/// Rewrite a relative `log4rs_file=...` value in the rendered INI to an
/// absolute path resolved from the orchestrator's current working directory.
///
/// The child process is launched with `current_dir(&run_dir)`, so any
/// relative path in the INI would otherwise be resolved against `run_dir`.
/// Without this rewrite the engine silently skips log4rs initialisation
/// (because `<run_dir>/log4rs.yaml` does not exist), which removes per-run
/// log isolation and leaves the resumer reading from the shared repo-level
/// `logs/finished.log`.
///
/// If the value is already absolute or cannot be canonicalised against the
/// orchestrator cwd, it is preserved verbatim.
fn absolutize_log4rs_path(ini: &str) -> String {
    let base = std::env::current_dir().ok();
    absolutize_log4rs_path_with_base(ini, base.as_deref())
}

fn absolutize_log4rs_path_with_base(ini: &str, base: Option<&Path>) -> String {
    let mut out = String::with_capacity(ini.len());
    let ends_with_newline = ini.ends_with('\n');
    let mut lines = ini.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("log4rs_file") {
            if let Some(eq_rest) = rest.trim_start().strip_prefix('=') {
                let value = eq_rest.trim();
                let path = std::path::Path::new(value);
                if path.is_relative() {
                    let candidate = match base {
                        Some(b) => b.join(path),
                        None => path.to_path_buf(),
                    };
                    if let Ok(abs) = std::fs::canonicalize(&candidate) {
                        let indent_len = line.len() - trimmed.len();
                        out.push_str(&line[..indent_len]);
                        out.push_str("log4rs_file=");
                        out.push_str(abs.to_string_lossy().as_ref());
                        if lines.peek().is_some() || ends_with_newline {
                            out.push('\n');
                        }
                        continue;
                    }
                }
            }
        }
        out.push_str(line);
        if lines.peek().is_some() || ends_with_newline {
            out.push('\n');
        }
    }
    out
}

/// Parse a key=value position string into a JSON object.
///
/// The engine writes position in formats like:
/// - `binlog_file=mysql-bin.000003,binlog_pos=154`  (MySQL CDC)
/// - `lsn=0/1A2B3C4D`  (Postgres CDC)
/// - `scn=12345`  (Oracle CDC)
fn parse_position_kv(s: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for pair in s.split(',') {
        if let Some((key, value)) = pair.split_once('=') {
            map.insert(
                key.trim().to_string(),
                serde_json::Value::String(value.trim().to_string()),
            );
        }
    }
    if map.is_empty() {
        serde_json::Value::String(s.to_string())
    } else {
        serde_json::Value::Object(map)
    }
}

/// Check process liveness by PID using `kill(pid, 0)`.
///
/// Returns `ChildStatus::Running` if the process exists,
/// or `ChildStatus::Exited` if it does not.
#[cfg(unix)]
fn pid_status(pid: u32) -> ChildStatus {
    if pid > 0 && unsafe { libc::kill(pid as i32, 0) == 0 } {
        ChildStatus::Running
    } else {
        ChildStatus::Exited(ExitStatus::Exited { code: -1 })
    }
}

/// Check process liveness by PID (non-Unix fallback).
///
/// Always returns `Exited` on non-Unix platforms since we cannot
/// reliably check PID liveness without `kill(pid, 0)`.
#[cfg(not(unix))]
fn pid_status(pid: u32) -> ChildStatus {
    let _ = pid;
    ChildStatus::Exited(ExitStatus::Exited { code: -1 })
}

/// Kill a re-attached process by PID with graceful shutdown.
///
/// 1. Sends SIGTERM to the PID.
/// 2. Polls via `kill(pid, 0)` up to `grace_secs` for the process to exit.
/// 3. If still alive, sends SIGKILL and waits briefly.
#[cfg(unix)]
async fn kill_reattached(pid: u32, grace_secs: u64) -> Result<KillResult, String> {
    send_sigterm(pid)?;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(grace_secs);
    let mut stop_method = "sigterm".to_string();

    loop {
        if unsafe { libc::kill(pid as i32, 0) != 0 } {
            return Ok(KillResult {
                stop_method,
                exit_status: ExitStatus::Exited { code: -1 },
            });
        }
        if tokio::time::Instant::now() >= deadline {
            send_sigkill(pid)?;
            stop_method = "sigkill".to_string();

            // Wait for SIGKILL to take effect.
            let kill_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(2);
            loop {
                if unsafe { libc::kill(pid as i32, 0) != 0 } {
                    return Ok(KillResult {
                        stop_method,
                        exit_status: ExitStatus::Signaled { signal: 9 },
                    });
                }
                if tokio::time::Instant::now() >= kill_deadline {
                    return Ok(KillResult {
                        stop_method,
                        exit_status: ExitStatus::Signaled { signal: 9 },
                    });
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

/// Kill a re-attached process by PID (non-Unix fallback).
#[cfg(not(unix))]
async fn kill_reattached(pid: u32, _grace_secs: u64) -> Result<KillResult, String> {
    Err(format!(
        "cannot kill re-attached process {pid} on non-Unix platform"
    ))
}

/// Send SIGTERM to a process by PID.
#[cfg(unix)]
fn send_sigterm(pid: u32) -> Result<(), String> {
    use std::process::Command;
    Command::new("kill")
        .args(["-s", "TERM", &pid.to_string()])
        .output()
        .map_err(|e| format!("failed to send SIGTERM to pid {pid}: {e}"))?;
    Ok(())
}

/// Send SIGKILL to a process by PID.
#[cfg(unix)]
fn send_sigkill(pid: u32) -> Result<(), String> {
    use std::process::Command;
    Command::new("kill")
        .args(["-s", "KILL", &pid.to_string()])
        .output()
        .map_err(|e| format!("failed to send SIGKILL to pid {pid}: {e}"))?;
    Ok(())
}

/// Send SIGTERM to a process by PID (non-Unix fallback).
#[cfg(not(unix))]
fn send_sigterm(pid: u32) -> Result<(), String> {
    Err("SIGTERM is not supported on this platform".to_string())
}

/// Send SIGKILL to a process by PID (non-Unix fallback).
#[cfg(not(unix))]
fn send_sigkill(pid: u32) -> Result<(), String> {
    Err("SIGKILL is not supported on this platform".to_string())
}

/// Convert `std::process::ExitStatus` to our `ExitStatus`.
fn exit_status_from_process(status: &std::process::ExitStatus) -> ExitStatus {
    if let Some(code) = status.code() {
        ExitStatus::Exited { code }
    } else {
        // On Unix, no code means killed by signal.
        ExitStatus::Signaled { signal: 9 }
    }
}

/// Shared state tracking active Runs across the application.
///
/// This is wrapped in `web::Data` and shared across all request handlers.
/// It maps task IDs to their active Run handles, ensuring at most one
/// active Run per Task.
#[derive(Debug, Clone, Default)]
pub struct RunRegistry {
    // task_id → run_id (for checking active runs; the actual child process
    // is managed internally by the lifecycle handlers).
    // We store just the mapping here; the RunHandle is consumed by the
    // background supervisor task.
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_binary_path_default() {
        // Without env var, should return default.
        std::env::remove_var(ENGINE_BINARY_ENV);
        assert_eq!(engine_binary_path(), DEFAULT_ENGINE_BINARY);
    }

    #[test]
    fn test_engine_binary_path_override() {
        std::env::set_var(ENGINE_BINARY_ENV, "/tmp/fake-engine");
        assert_eq!(engine_binary_path(), "/tmp/fake-engine");
        std::env::remove_var(ENGINE_BINARY_ENV);
    }

    #[test]
    fn test_grace_window_default() {
        std::env::remove_var(GRACE_WINDOW_ENV);
        assert_eq!(grace_window_secs(), DEFAULT_GRACE_WINDOW_SECS);
    }

    #[test]
    fn test_run_data_dir_default() {
        std::env::remove_var(RUN_DATA_DIR_ENV);
        assert_eq!(run_data_dir(), DEFAULT_RUN_DATA_DIR);
    }

    #[test]
    fn test_absolutize_log4rs_path_relative_existing() {
        let tmp = std::env::temp_dir().join(format!("absolutize-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let log4rs = tmp.join("log4rs.yaml");
        std::fs::write(&log4rs, "appenders: {}\n").unwrap();

        let ini = "[runtime]\nlog_level=info\nlog4rs_file=./log4rs.yaml\nlog_dir=./logs\n";
        let out = absolutize_log4rs_path_with_base(ini, Some(&tmp));

        let abs = std::fs::canonicalize(&log4rs).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);

        assert!(
            out.contains(&format!("log4rs_file={}", abs.to_string_lossy())),
            "expected absolute log4rs_file in {out}",
        );
        assert!(
            out.contains("log_dir=./logs"),
            "log_dir must remain relative: {out}"
        );
        assert!(out.contains("log_level=info"));
    }

    #[test]
    fn test_absolutize_log4rs_path_absolute_unchanged() {
        let ini = "[runtime]\nlog4rs_file=/etc/ape-dts/log4rs.yaml\nlog_dir=./logs\n";
        let out = absolutize_log4rs_path_with_base(ini, Some(std::path::Path::new("/tmp")));
        assert_eq!(out, ini);
    }

    #[test]
    fn test_absolutize_log4rs_path_missing_relative_unchanged() {
        let tmp = std::env::temp_dir().join(format!("absolutize-miss-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();

        let ini = "[runtime]\nlog4rs_file=./log4rs.yaml\nlog_dir=./logs\n";
        let out = absolutize_log4rs_path_with_base(ini, Some(&tmp));

        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(out, ini, "missing relative path should be left untouched");
    }

    #[test]
    fn test_absolutize_log4rs_path_preserves_other_keys() {
        let tmp = std::env::temp_dir().join(format!("absolutize-keep-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("log4rs.yaml"), "appenders: {}\n").unwrap();

        let ini = "[extractor]\nurl=mysql://x\n[runtime]\nlog4rs_file=./log4rs.yaml\nlog_dir=./logs\n[resumer]\nlog_dir=./logs\n";
        let out = absolutize_log4rs_path_with_base(ini, Some(&tmp));

        let _ = std::fs::remove_dir_all(&tmp);

        assert!(out.contains("url=mysql://x"));
        assert!(out.contains("[resumer]"));
        // log_dir lines untouched (they need to remain ./logs so child cwd resolves them).
        let log_dir_lines = out.lines().filter(|l| l.starts_with("log_dir=")).count();
        assert_eq!(log_dir_lines, 2);
    }

    #[test]
    fn test_parse_position_kv_mysql() {
        let val = parse_position_kv("binlog_file=mysql-bin.000003,binlog_pos=154");
        assert_eq!(val["binlog_file"], "mysql-bin.000003");
        assert_eq!(val["binlog_pos"], "154");
    }

    #[test]
    fn test_parse_position_kv_lsn() {
        let val = parse_position_kv("lsn=0/1A2B3C4D");
        assert_eq!(val["lsn"], "0/1A2B3C4D");
    }

    #[test]
    fn test_parse_position_kv_scn() {
        let val = parse_position_kv("scn=12345");
        assert_eq!(val["scn"], "12345");
    }

    #[test]
    fn test_parse_position_kv_empty() {
        let val = parse_position_kv("");
        // Empty input should produce a string, not an empty object.
        assert!(val.is_string());
    }

    #[test]
    fn test_read_position_missing_file() {
        let dir = std::env::temp_dir().join("test-no-position");
        let _ = std::fs::remove_dir_all(&dir);
        let result = LocalExecutor::read_position(&dir);
        assert!(result.is_none());
    }

    #[test]
    fn test_read_position_with_content() {
        let dir = std::env::temp_dir().join("test-position-content");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        std::fs::write(dir.join("logs/position.log"), "lsn=0/1A2B3C4D\n").unwrap();
        let result = LocalExecutor::read_position(&dir);
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(val["lsn"], "0/1A2B3C4D");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_position_empty_file() {
        let dir = std::env::temp_dir().join("test-position-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        std::fs::write(dir.join("logs/position.log"), "").unwrap();
        let result = LocalExecutor::read_position(&dir);
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_spawn_with_fake_binary() {
        // Use "sleep" as a fake engine binary — it accepts a numeric arg
        // and runs for that many seconds. The first positional arg is the
        // INI path, but sleep will interpret it as duration; we don't care
        // about the sleep duration for this test.
        let run_id = format!("test-{}", uuid::Uuid::new_v4());
        let ini_content = "[global]\ntask_id=test\n";

        let result = LocalExecutor::spawn(&run_id, ini_content, Some("sleep")).await;
        assert!(result.is_ok(), "spawn should succeed with 'sleep' binary");

        let handle = result.unwrap();
        assert!(handle.pid > 0, "spawned child should have a PID");

        // Verify directory structure was created.
        let run_dir = &handle.run_dir;
        assert!(run_dir.exists(), "run directory should exist");
        assert!(
            run_dir.join("task_config.ini").exists(),
            "INI file should exist"
        );
        assert!(run_dir.join("logs").exists(), "logs directory should exist");

        // Check status while running.
        let status = LocalExecutor::status(&handle).await;
        assert!(
            matches!(status, ChildStatus::Running),
            "child should be running"
        );

        // Kill the child.
        let kill_result = LocalExecutor::kill_with_grace(&handle, 2).await;
        assert!(kill_result.is_ok(), "kill should succeed");
        let kr = kill_result.unwrap();
        assert_eq!(kr.stop_method, "sigterm", "should use SIGTERM first");

        // Clean up.
        let _ = std::fs::remove_dir_all(run_dir);
    }

    #[tokio::test]
    async fn test_spawn_writes_ini_to_cwd() {
        let run_id = format!("test-ini-{}", uuid::Uuid::new_v4());
        let ini_content = "[global]\ntask_id=ini_test\n[extractor]\ndb_type=mysql\n";

        let handle = LocalExecutor::spawn(&run_id, ini_content, Some("sleep"))
            .await
            .unwrap();

        // Read back the INI and verify.
        let ini_path = handle.run_dir.join("task_config.ini");
        let content = std::fs::read_to_string(&ini_path).unwrap();
        assert_eq!(content, ini_content, "INI content should match exactly");

        // Kill and clean up.
        let _ = LocalExecutor::kill_with_grace(&handle, 2).await;
        let _ = std::fs::remove_dir_all(&handle.run_dir);
    }

    #[tokio::test]
    async fn test_spawn_binary_path_override() {
        let run_id = format!("test-override-{}", uuid::Uuid::new_v4());
        let ini_content = "[global]\ntask_id=override_test\n";

        // Override to "sleep" instead of the default engine binary.
        let handle = LocalExecutor::spawn(&run_id, ini_content, Some("sleep"))
            .await
            .unwrap();

        assert!(handle.pid > 0);

        let _ = LocalExecutor::kill_with_grace(&handle, 2).await;
        let _ = std::fs::remove_dir_all(&handle.run_dir);
    }

    #[tokio::test]
    async fn test_spawn_canonicalizes_relative_binary_path() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Create a stub script at a path *relative to the parent cwd*.
        // Without canonicalization, current_dir(&run_dir) on the spawn
        // would make execve fail because the relative path no longer
        // resolves to an existing file.
        let stubs_dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-stubs");
        std::fs::create_dir_all(&stubs_dir).unwrap();
        let stub_name = format!("stub-{}.sh", uuid::Uuid::new_v4());
        let stub_path = stubs_dir.join(&stub_name);
        {
            let mut f = std::fs::File::create(&stub_path).unwrap();
            writeln!(f, "#!/bin/sh\nsleep 5").unwrap();
        }
        let mut perms = std::fs::metadata(&stub_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub_path, perms).unwrap();

        let rel_path = format!("target/test-stubs/{stub_name}");

        let run_id = format!("test-canon-{}", uuid::Uuid::new_v4());
        let result = LocalExecutor::spawn(&run_id, "[g]\n", Some(&rel_path)).await;

        let _ = std::fs::remove_file(&stub_path);

        let handle = result.expect("spawn with relative binary path should succeed");
        let _ = LocalExecutor::kill_with_grace(&handle, 2).await;
        let _ = std::fs::remove_dir_all(&handle.run_dir);
    }

    #[tokio::test]
    async fn test_spawn_missing_binary_returns_error() {
        let run_id = format!("test-missing-{}", uuid::Uuid::new_v4());
        let ini_content = "[global]\ntask_id=missing_test\n";

        let result =
            LocalExecutor::spawn(&run_id, ini_content, Some("/nonexistent/path/to/engine")).await;

        assert!(result.is_err(), "spawning a missing binary should fail");
        let _ = std::fs::remove_dir_all(PathBuf::from(run_data_dir()).join(&run_id));
    }

    #[tokio::test]
    async fn test_kill_terminates_child_within_5s() {
        let run_id = format!("test-kill-{}", uuid::Uuid::new_v4());
        let ini_content = "[global]\ntask_id=kill_test\n";

        // Spawn with sleep 30 (long-running).
        let handle = LocalExecutor::spawn(&run_id, ini_content, Some("sleep"))
            .await
            .unwrap();

        let pid = handle.pid;

        // Kill should terminate within 5s (grace window = 3s for this test).
        let start = std::time::Instant::now();
        let kill_result = LocalExecutor::kill_with_grace(&handle, 3).await;
        let elapsed = start.elapsed();

        assert!(kill_result.is_ok(), "kill should succeed");
        assert!(
            elapsed.as_secs() < 5,
            "kill should complete within 5s, took {:?}",
            elapsed
        );

        // Verify process is gone.
        // On Unix, kill -0 checks if a process exists.
        #[cfg(unix)]
        {
            let check = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output();
            assert!(
                check.is_err() || !check.unwrap().status.success(),
                "process {pid} should be gone after kill"
            );
        }

        let _ = std::fs::remove_dir_all(&handle.run_dir);
    }

    #[tokio::test]
    async fn test_status_after_exit() {
        let run_id = format!("test-status-exit-{}", uuid::Uuid::new_v4());
        let ini_content = "[global]\ntask_id=status_test\n";

        // Spawn with "true" which exits immediately with code 0.
        let handle = LocalExecutor::spawn(&run_id, ini_content, Some("true"))
            .await
            .unwrap();

        // Wait briefly for the process to exit.
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let status = LocalExecutor::status(&handle).await;
        match status {
            ChildStatus::Exited(ExitStatus::Exited { code }) => {
                assert_eq!(code, 0, "true should exit with code 0");
            }
            other => panic!("expected Exited with code 0, got {:?}", other),
        }

        let _ = std::fs::remove_dir_all(&handle.run_dir);
    }

    #[tokio::test]
    async fn test_failed_run_records_nonzero_exit() {
        let run_id = format!("test-failed-{}", uuid::Uuid::new_v4());
        let ini_content = "[global]\ntask_id=failed_test\n";

        // Spawn with "false" which exits with code 1.
        let handle = LocalExecutor::spawn(&run_id, ini_content, Some("false"))
            .await
            .unwrap();

        // Wait for the process to exit.
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let status = LocalExecutor::status(&handle).await;
        match status {
            ChildStatus::Exited(ExitStatus::Exited { code }) => {
                assert_ne!(code, 0, "false should exit with non-zero code");
            }
            other => panic!("expected Exited with non-zero code, got {:?}", other),
        }

        let _ = std::fs::remove_dir_all(&handle.run_dir);
    }

    #[test]
    fn test_reattach_creates_handle_with_correct_fields() {
        let run_id = "reattach-test-run";
        let pid = 12345u32;
        let run_dir = PathBuf::from("/data/runs/reattach-test-run");

        let handle = LocalExecutor::reattach(run_id, pid, run_dir.clone());

        assert_eq!(handle.run_id, run_id);
        assert_eq!(handle.pid, pid);
        assert_eq!(handle.run_dir, run_dir);
        assert!(handle.reattached, "reattached flag should be true");
    }

    #[test]
    fn test_reattach_handle_clones_correctly() {
        let run_id = "reattach-clone-test";
        let pid = 54321u32;
        let run_dir = PathBuf::from("/data/runs/reattach-clone-test");

        let handle = LocalExecutor::reattach(run_id, pid, run_dir.clone());
        let clone = handle.clone();

        assert_eq!(clone.run_id, run_id);
        assert_eq!(clone.pid, pid);
        assert_eq!(clone.run_dir, run_dir);
        assert!(clone.reattached);
    }

    #[tokio::test]
    async fn test_status_reattached_alive_pid_reports_running() {
        // Spawn a real "sleep" process so we have a valid PID.
        let handle = LocalExecutor::spawn(
            &format!("reattach-alive-{}", uuid::Uuid::new_v4()),
            "[global]\ntask_id=test\n",
            Some("sleep"),
        )
        .await
        .unwrap();

        let pid = handle.pid;
        let run_dir = handle.run_dir.clone();

        // Kill the original child so we can create a re-attached handle
        // for the same PID (the process is still alive).
        // We must NOT use kill_with_grace because that consumes the child.
        // Instead, just verify the PID is alive, then construct a re-attached handle.
        let reattach_handle = LocalExecutor::reattach(&handle.run_id, pid, run_dir);

        let status = LocalExecutor::status(&reattach_handle).await;
        assert!(
            matches!(status, ChildStatus::Running),
            "reattached handle with alive PID should report Running"
        );

        // Clean up: kill the original handle
        let _ = LocalExecutor::kill_with_grace(&handle, 2).await;
        let _ = std::fs::remove_dir_all(&handle.run_dir);
    }

    #[tokio::test]
    async fn test_status_reattached_dead_pid_reports_exited() {
        // Use a PID that definitely doesn't exist (very high PID).
        let dead_pid = 4000000u32;
        let handle = LocalExecutor::reattach(
            "reattach-dead-test",
            dead_pid,
            PathBuf::from("/data/runs/nonexistent"),
        );

        let status = LocalExecutor::status(&handle).await;
        assert!(
            matches!(status, ChildStatus::Exited(_)),
            "reattached handle with dead PID should report Exited"
        );
    }

    #[tokio::test]
    async fn test_kill_reattached_terminates_process() {
        // Spawn a real "sleep" process, then re-attach and kill via the
        // re-attached handle.
        let spawned = LocalExecutor::spawn(
            &format!("reattach-kill-{}", uuid::Uuid::new_v4()),
            "[global]\ntask_id=test\n",
            Some("sleep"),
        )
        .await
        .unwrap();

        let pid = spawned.pid;
        let run_dir = spawned.run_dir.clone();

        // Create a re-attached handle for the same PID.
        let reattach_handle = LocalExecutor::reattach(&spawned.run_id, pid, run_dir);

        // Kill via the re-attached handle.
        let result = LocalExecutor::kill_with_grace(&reattach_handle, 3).await;
        assert!(result.is_ok(), "kill re-attached process should succeed");

        let kr = result.unwrap();
        assert!(
            kr.stop_method == "sigterm" || kr.stop_method == "sigkill",
            "stop method should be sigterm or sigkill, got {}",
            kr.stop_method
        );

        // Also reap the original child (zombie) so it doesn't linger.
        let _ = LocalExecutor::kill_with_grace(&spawned, 2).await;

        // Verify process is truly gone after reaping.
        #[cfg(unix)]
        {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
            assert!(!alive, "process {pid} should be gone after kill");
        }

        let _ = std::fs::remove_dir_all(&spawned.run_dir);
    }
}
