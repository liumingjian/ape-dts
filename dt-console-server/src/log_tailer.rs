//! LogTailer — tails per-Run log files and emits chunks.
//!
//! Supports the seven known log file names: default, position, monitor,
//! finished, task, http, commit. Uses polling to detect new content.
//! Handles file rotation/truncation by restarting from offset 0.

use chrono::Utc;
use regex::{Captures, Regex};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

/// The seven known log file names produced by the engine.
pub const KNOWN_LOG_FILES: &[&str] = &[
    "default", "position", "monitor", "finished", "task", "http", "commit",
];

/// Default polling interval for log tailing.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 250;

/// Structured payload emitted for each named SSE `log` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StructuredLogLine {
    pub timestamp: String,
    pub level: String,
    pub source: String,
    pub file: String,
    pub message: String,
}

/// Parse one engine log line into the public Run-log wire contract.
pub fn parse_log_line(file: &str, raw: &str) -> StructuredLogLine {
    let pattern = Regex::new(
        r"^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?) - (DEBUG|INFO|WARN|ERROR) - (?:\[([^\]]+)\] - )?(.*)$",
    )
    .expect("static log pattern");

    let (timestamp, level, source, message) = if let Some(captures) = pattern.captures(raw) {
        let timestamp = format!(
            "{}Z",
            captures
                .get(1)
                .expect("timestamp capture")
                .as_str()
                .replace(' ', "T")
        );
        let level = captures.get(2).expect("level capture").as_str().to_string();
        let source = captures
            .get(3)
            .map_or("dt-main", |value| value.as_str())
            .to_string();
        let message = captures
            .get(4)
            .expect("message capture")
            .as_str()
            .to_string();
        (timestamp, level, source, message)
    } else {
        (
            Utc::now().to_rfc3339(),
            infer_log_level(raw).to_string(),
            "dt-main".to_string(),
            raw.to_string(),
        )
    };

    StructuredLogLine {
        timestamp,
        level,
        source,
        file: format!("{file}.log"),
        message: redact_log_text(&message),
    }
}

fn infer_log_level(line: &str) -> &'static str {
    for (level, name) in [
        (LogLevel::Error, "ERROR"),
        (LogLevel::Warn, "WARN"),
        (LogLevel::Debug, "DEBUG"),
        (LogLevel::Info, "INFO"),
    ] {
        if level.matches_line(line) {
            return name;
        }
    }
    "INFO"
}

/// Redact common credential forms before log content leaves the server.
pub fn redact_log_text(text: &str) -> String {
    let url_credentials = Regex::new(r"(?i)([a-z][a-z0-9+.-]*://[^\s/:@]+:)[^\s@]+(@)")
        .expect("static URL credential pattern");
    let key_values = Regex::new(
        r"(?i)\b(password|passwd|pwd|token|secret|access_key|secret_key)\s*=\s*([^\s,;]+)",
    )
    .expect("static key-value secret pattern");
    let authorization = Regex::new(r"(?i)(authorization\s*[:=]\s*(?:bearer\s+)?)([^\s,;]+)")
        .expect("static authorization pattern");

    let redacted = url_credentials.replace_all(text, "$1***$2");
    let redacted = key_values.replace_all(&redacted, |captures: &Captures<'_>| {
        format!("{}=***", &captures[1])
    });
    authorization.replace_all(&redacted, "$1***").into_owned()
}

/// A chunk of log data read from a Run's log file.
#[derive(Debug, Clone)]
pub struct LogChunk {
    /// The log file name (e.g. "default", "position").
    pub file: String,
    /// The line content (without trailing newline).
    pub line: String,
    /// Monotonic sequence number within this tail session.
    pub seq: u64,
}

/// Check if a file name is a known log file.
pub fn is_known_log_file(name: &str) -> bool {
    KNOWN_LOG_FILES.contains(&name)
}

/// Sanitise a log file name: reject path traversal and unknown names.
///
/// Returns `Ok(name)` if the name is a known log file.
/// Returns `Err(reason)` if the name contains path separators,
/// parent directory references, or is not a known log file.
pub fn sanitise_log_file_name(name: &str) -> Result<String, String> {
    // Reject empty
    if name.is_empty() {
        return Err("log file name must not be empty".to_string());
    }

    // Reject path traversal characters
    if name.contains('/') || name.contains('\\') || name.contains('.') {
        return Err(format!("log file name contains invalid characters: {name}"));
    }

    // URL-decoded variants are handled at the HTTP layer, but let's also
    // check for common traversal patterns after percent-decoding
    if name.contains("%2E") || name.contains("%2F") || name.contains("%5C") {
        return Err(format!("log file name contains encoded traversal: {name}"));
    }

    // Must be a known log file
    if !is_known_log_file(name) {
        return Err(format!("unknown log file: {name}"));
    }

    Ok(name.to_string())
}

/// Resolve the full path to a log file within a Run's log directory.
///
/// Returns `None` if the file name fails sanitisation.
pub fn resolve_log_path(log_dir: &Path, file_name: &str) -> Option<PathBuf> {
    sanitise_log_file_name(file_name).ok().map(|name| {
        // Always append .log extension
        log_dir.join(format!("{name}.log"))
    })
}

/// Tail a single log file, producing a stream of LogChunks.
///
/// This function opens the file at the given path, seeks to `start_offset`,
/// and then polls for new content every `poll_interval`. Each new line
/// produces one `LogChunk`. File truncation (rotation) is detected by
/// comparing file size to the current offset; if the file shrinks,
/// we restart from offset 0.
///
/// The stream ends when `until` resolves (typically on client disconnect).
pub async fn tail_file(
    file_name: &str,
    file_path: PathBuf,
    start_offset: u64,
    poll_interval: Duration,
    until: tokio::sync::watch::Receiver<bool>,
    tx: tokio::sync::mpsc::Sender<LogChunk>,
) {
    let mut offset = start_offset;
    let mut seq: u64 = 0;
    let mut prev_len: u64 = 0;

    loop {
        // Check if we should stop
        if *until.borrow() {
            break;
        }

        // Try to open and read the file
        match read_new_lines(&file_path, &mut offset, &mut prev_len).await {
            Ok(lines) => {
                for line in lines {
                    seq += 1;
                    let chunk = LogChunk {
                        file: file_name.to_string(),
                        line,
                        seq,
                    };
                    if tx.send(chunk).await.is_err() {
                        // Receiver dropped — client disconnected
                        return;
                    }
                }
            }
            Err(e) => {
                // File might not exist yet (Run just started). Just wait.
                tracing::debug!("log tail read error for {:?}: {e}", file_path);
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Read new lines from a file starting at the given offset.
///
/// Detects file truncation by checking if the file size decreased.
/// On truncation, resets offset to 0 and reads from the start.
async fn read_new_lines(
    path: &Path,
    offset: &mut u64,
    prev_len: &mut u64,
) -> Result<Vec<String>, std::io::Error> {
    let metadata = tokio::fs::metadata(path).await?;
    let current_len = metadata.len();

    // Detect truncation: file got smaller since we last checked
    if current_len < *offset || (*prev_len > 0 && current_len < *prev_len) {
        *offset = 0;
    }

    *prev_len = current_len;

    if *offset >= current_len {
        return Ok(Vec::new());
    }

    let mut file = tokio::fs::File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(*offset)).await?;

    let mut buf = String::new();
    let mut new_lines = Vec::new();

    // Read in chunks until EOF
    let mut total_read = 0u64;
    let mut read_buf = [0u8; 8192];
    loop {
        let n = match file.read(&mut read_buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(e),
        };

        let s = String::from_utf8_lossy(&read_buf[..n]);
        buf.push_str(&s);
        total_read += n as u64;
    }

    *offset += total_read;

    // Split into lines
    for line in buf.lines() {
        let trimmed = line.trim_end_matches('\r').trim_end_matches('\n');
        if !trimmed.is_empty() {
            // Replace non-UTF-8 bytes with replacement character
            new_lines.push(trimmed.to_string());
        }
    }

    Ok(new_lines)
}

/// Log level prefix patterns for filtering.
///
/// Engine log lines typically start with a level prefix like
/// `[INFO]`, `[WARN]`, `[ERROR]`, `[DEBUG]`, or `LEVEL:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    /// Parse a log level from a query parameter string.
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "ERROR" | "ERR" => Some(LogLevel::Error),
            "WARN" | "WARNING" => Some(LogLevel::Warn),
            "INFO" => Some(LogLevel::Info),
            "DEBUG" | "DBG" => Some(LogLevel::Debug),
            _ => None,
        }
    }

    /// Check if a log line matches this level.
    ///
    /// Matches common patterns:
    /// - `[ERROR] message`
    /// - `ERROR: message`
    /// - `[level=ERROR] message`
    pub fn matches_line(&self, line: &str) -> bool {
        let upper = line.to_uppercase();
        let level_str = match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
        };

        // Check bracket form: [ERROR], [WARN], etc.
        if upper.contains(&format!("[{level_str}]")) {
            return true;
        }

        // Check prefix form: ERROR:, WARN:, etc.
        if upper.starts_with(&format!("{level_str}:"))
            || upper.starts_with(&format!("{level_str} "))
        {
            return true;
        }

        // Check key=value form: level=ERROR, level=WARN, etc.
        if upper.contains(&format!("LEVEL={level_str}"))
            || upper.contains(&format!("LEVEL={level_str},"))
        {
            return true;
        }

        false
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structured_log_line_uses_wire_contract_and_redacts_secrets() {
        let line = parse_log_line(
            "default",
            "2026-07-18 12:34:56.789 - ERROR - [extractor] - connect mysql://root:supersecret@db password=hunter2 token=abc123",
        );

        assert_eq!(line.timestamp, "2026-07-18T12:34:56.789Z");
        assert_eq!(line.level, "ERROR");
        assert_eq!(line.source, "extractor");
        assert_eq!(line.file, "default.log");
        assert_eq!(
            line.message,
            "connect mysql://root:***@db password=*** token=***"
        );
        let json = serde_json::to_string(&line).unwrap();
        assert!(!json.contains("supersecret"));
        assert!(!json.contains("hunter2"));
        assert!(!json.contains("abc123"));
    }

    #[test]
    fn test_structured_log_line_defaults_unknown_format() {
        let line = parse_log_line("monitor", "replication is idle");
        assert_eq!(line.level, "INFO");
        assert_eq!(line.source, "dt-main");
        assert_eq!(line.file, "monitor.log");
        assert_eq!(line.message, "replication is idle");
        assert!(!line.timestamp.is_empty());
    }

    #[test]
    fn test_known_log_files() {
        assert!(is_known_log_file("default"));
        assert!(is_known_log_file("position"));
        assert!(is_known_log_file("monitor"));
        assert!(is_known_log_file("finished"));
        assert!(is_known_log_file("task"));
        assert!(is_known_log_file("http"));
        assert!(is_known_log_file("commit"));
        assert!(!is_known_log_file("nope"));
        assert!(!is_known_log_file(""));
    }

    #[test]
    fn test_sanitise_rejects_traversal() {
        // Path traversal
        assert!(sanitise_log_file_name("../../etc/passwd").is_err());
        assert!(sanitise_log_file_name("..\\..\\etc\\passwd").is_err());
        assert!(sanitise_log_file_name("default/../../etc").is_err());

        // Dot in name
        assert!(sanitise_log_file_name("default.log").is_err());

        // Unknown file
        assert!(sanitise_log_file_name("nope").is_err());

        // Empty
        assert!(sanitise_log_file_name("").is_err());

        // Valid
        assert_eq!(sanitise_log_file_name("default").unwrap(), "default");
        assert_eq!(sanitise_log_file_name("position").unwrap(), "position");
    }

    #[test]
    fn test_resolve_log_path() {
        let dir = PathBuf::from("/data/runs/abc/logs");
        let path = resolve_log_path(&dir, "default");
        assert_eq!(path, Some(PathBuf::from("/data/runs/abc/logs/default.log")));

        let path = resolve_log_path(&dir, "../../etc/passwd");
        assert_eq!(path, None);
    }

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::from_str_opt("ERROR"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str_opt("error"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str_opt("WARN"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_str_opt("INFO"), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_str_opt("DEBUG"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::from_str_opt("nope"), None);
    }

    #[test]
    fn test_log_level_matches_bracket_form() {
        let level = LogLevel::Error;
        assert!(level.matches_line("[ERROR] connection failed"));
        assert!(level.matches_line("[error] lowercase"));
        assert!(!level.matches_line("[INFO] something"));
    }

    #[test]
    fn test_log_level_matches_prefix_form() {
        let level = LogLevel::Warn;
        assert!(level.matches_line("WARN: something bad"));
        assert!(level.matches_line("WARN something bad"));
        assert!(!level.matches_line("ERROR: something"));
    }

    #[test]
    fn test_log_level_matches_kv_form() {
        let level = LogLevel::Info;
        assert!(level.matches_line("level=INFO extractor_rps=42"));
        assert!(level.matches_line("level=INFO,other=val"));
        assert!(!level.matches_line("level=ERROR"));
    }

    #[tokio::test]
    async fn test_tail_file_emits_chunks() {
        let dir = std::env::temp_dir().join("test-log-tailer-chunks");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let log_path = dir.join("default.log");
        std::fs::write(&log_path, "hello\nworld\n").unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let (_, until_rx) = tokio::sync::watch::channel(false);

        let path = log_path.clone();
        let handle = tokio::spawn(async move {
            tail_file("default", path, 0, Duration::from_millis(50), until_rx, tx).await;
        });

        // Collect chunks
        let mut chunks = Vec::new();
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Ok(chunk) = rx.try_recv() {
            chunks.push(chunk);
        }

        assert!(
            chunks.len() >= 2,
            "should have at least 2 chunks, got {}",
            chunks.len()
        );
        assert_eq!(chunks[0].line, "hello");
        assert_eq!(chunks[1].line, "world");

        handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_tail_file_detects_new_lines() {
        let dir = std::env::temp_dir().join("test-log-tailer-new-lines");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let log_path = dir.join("default.log");
        std::fs::write(&log_path, "first\n").unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let (stop_tx, until_rx) = tokio::sync::watch::channel(false);

        let path = log_path.clone();
        let handle = tokio::spawn(async move {
            tail_file("default", path, 0, Duration::from_millis(50), until_rx, tx).await;
        });

        // Wait for first chunk
        tokio::time::sleep(Duration::from_millis(200)).await;
        let first = rx.try_recv().ok();
        assert!(first.is_some());
        assert_eq!(first.unwrap().line, "first");

        // Append a new line
        std::fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .unwrap()
            .write_all(b"second\n")
            .unwrap();

        // Wait for new chunk
        tokio::time::sleep(Duration::from_millis(300)).await;
        let second = rx.try_recv().ok();
        assert!(second.is_some(), "should detect new line");
        assert_eq!(second.unwrap().line, "second");

        stop_tx.send(true).unwrap();
        handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    use std::io::Write;

    #[tokio::test]
    async fn test_tail_file_handles_truncation() {
        let dir = std::env::temp_dir().join("test-log-tailer-truncation");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let log_path = dir.join("default.log");
        std::fs::write(&log_path, "old line 1\nold line 2\n").unwrap();

        // Read all existing lines first (simulating prior tail)
        let mut offset = 0u64;
        let mut prev_len = 0u64;
        let lines = read_new_lines(&log_path, &mut offset, &mut prev_len)
            .await
            .unwrap();
        assert_eq!(lines, vec!["old line 1", "old line 2"]);
        assert!(offset > 0);

        // Truncate and write new content
        std::fs::write(&log_path, "new content\n").unwrap();

        // After truncation, read_new_lines should detect file shrink
        // and restart from offset 0
        let lines = read_new_lines(&log_path, &mut offset, &mut prev_len)
            .await
            .unwrap();
        assert_eq!(lines, vec!["new content"]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
