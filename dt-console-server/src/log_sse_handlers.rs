//! SSE log stream handler and SsePublisher.
//!
//! - GET /api/runs/:id/logs/stream?file=<name>&level=<level>
//!   Produces SSE events for new log lines, with per-stream rate limiting,
//!   burst coalescing, heartbeat comments, and Last-Event-Id reconnect.
//!
//! - GET /api/runs/:id/logs?file=<name>
//!   Returns the full content of a single log file.

use actix_web::{get, web, HttpRequest, HttpResponse, Responder, ResponseError};
use actix_web_lab::sse::{Data, Event as SseEvent, Sse};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::error::{codes, ApiError};
use crate::log_tailer::{self, sanitise_log_file_name, LogLevel};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::{Run, UserContext};
use crate::repositories::run_repository::RunRepository;
use crate::repositories::task_repository::TaskRepository;

/// Default rate limit: max events per second per SSE subscriber.
const DEFAULT_RATE_LIMIT_PER_SEC: u32 = 500;

/// Default heartbeat interval in seconds.
const DEFAULT_HEARTBEAT_INTERVAL_SECS: u64 = 30;

/// Default retry hint in milliseconds for SSE reconnect.
const DEFAULT_RETRY_HINT_MS: u64 = 5000;

/// SSE event buffer size for Last-Event-Id reconnect.
const EVENT_BUFFER_SIZE: usize = 1000;

/// Shared state for active SSE log subscriptions.
///
/// Tracks per-Run LogTailer instances so that multiple subscribers to the
/// same (run_id, file) pair share a single tailer.
#[derive(Debug, Clone, Default)]
pub struct LogSseState {
    /// Active tailer handles: (run_id, file_name) → TailerEntry
    tailers: Arc<Mutex<std::collections::HashMap<String, TailerEntry>>>,
}

/// Entry for an active tailer.
#[derive(Debug)]
#[allow(dead_code)]
struct TailerEntry {
    /// Channel to signal the tailer to stop.
    stop_tx: tokio::sync::watch::Sender<bool>,
    /// Number of active subscribers.
    subscriber_count: usize,
    /// Broadcast sender for log chunks.
    chunk_tx: tokio::sync::broadcast::Sender<LogChunkWithId>,
}

/// A log chunk with a monotonic event ID.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LogChunkWithId {
    /// Monotonic event ID.
    event_id: u64,
    /// The file name.
    file: String,
    /// The line content (may contain multiple lines when coalesced).
    lines: Vec<String>,
}

/// Per-subscriber rate limiter state.
#[derive(Debug)]
struct SubscriberRateLimit {
    /// Events emitted in the current window.
    events_in_window: u32,
    /// Start of the current window.
    window_start: std::time::Instant,
    /// Maximum events per second.
    max_per_sec: u32,
}

impl SubscriberRateLimit {
    fn new(max_per_sec: u32) -> Self {
        Self {
            events_in_window: 0,
            window_start: std::time::Instant::now(),
            max_per_sec,
        }
    }

    /// Try to emit an event. Returns true if allowed immediately,
    /// false if rate-limited (line should be buffered for coalescing).
    fn check(&mut self) -> bool {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.window_start);

        // Reset window every second
        if elapsed >= Duration::from_secs(1) {
            self.events_in_window = 0;
            self.window_start = now;
        }

        if self.events_in_window < self.max_per_sec {
            self.events_in_window += 1;
            true
        } else {
            false
        }
    }
}

/// GET /api/runs/:id/logs/stream — SSE log stream.
///
/// Query parameters:
/// - `file` — log file name (required, one of the 7 known names)
/// - `level` — log level filter (optional, e.g. "ERROR")
///
/// SSE protocol:
/// - Each event carries `id:` and `data:`
/// - `retry:` hint on initial connect
/// - Heartbeat comments every 30 seconds
/// - `Last-Event-Id` header for reconnect
#[get("/runs/{id}/logs/stream")]
pub async fn log_stream(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    query: web::Query<LogStreamQuery>,
    sse_state: web::Data<LogSseState>,
    req: HttpRequest,
) -> HttpResponse {
    // RBAC: viewer and above can subscribe to logs
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let run_id = path.into_inner();
    let file_name = query.file.as_deref().unwrap_or("default");
    let level_filter = query.level.as_deref().and_then(LogLevel::from_str_opt);

    // Validate file name (rejects path traversal)
    let file_name = match sanitise_log_file_name(file_name) {
        Ok(name) => name,
        Err(reason) => {
            return ApiError::with_details(
                codes::UNKNOWN_LOG_FILE,
                reason,
                serde_json::json!({ "file": file_name }),
            )
            .error_response();
        }
    };

    // Look up the Run
    let run = match RunRepository::find_by_id(&pool, &run_id).await {
        Ok(r) => r,
        Err(_) => {
            return ApiError::with_details(
                codes::RUN_NOT_FOUND,
                "Run not found",
                serde_json::json!({ "id": run_id }),
            )
            .error_response();
        }
    };

    // RG ownership enforcement
    if let Err(e) = enforce_rg_ownership(&user, &run, &pool).await {
        return e.error_response();
    }

    // Resolve log directory
    let log_dir = match run.log_dir {
        Some(ref d) => PathBuf::from(d),
        None => {
            return ApiError::with_details(
                codes::RUN_NOT_FOUND,
                "Run has no log directory",
                serde_json::json!({ "run_id": run_id }),
            )
            .error_response();
        }
    };

    let log_path = match log_tailer::resolve_log_path(&log_dir, &file_name) {
        Some(p) => p,
        None => {
            return ApiError::with_details(
                codes::UNKNOWN_LOG_FILE,
                "Invalid log file name",
                serde_json::json!({ "file": file_name }),
            )
            .error_response();
        }
    };

    // Parse Last-Event-Id for reconnect
    let last_event_id = req
        .headers()
        .get("Last-Event-Id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    // Create SSE event stream using mpsc channel
    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<SseEvent>(100);

    // Spawn the stream producer task
    let producer_run_id = run_id.clone();
    let producer_file_name = file_name.clone();
    let producer_sse_state = sse_state.get_ref().clone();
    tokio::spawn(async move {
        produce_sse_events(
            producer_run_id,
            producer_file_name,
            log_path,
            level_filter,
            last_event_id,
            producer_sse_state,
            event_tx,
        )
        .await;
    });

    // Build SSE response with retry hint and keep-alive
    let sse = Sse::from_infallible_receiver(event_rx)
        .with_retry_duration(Duration::from_millis(DEFAULT_RETRY_HINT_MS))
        .with_keep_alive(Duration::from_secs(DEFAULT_HEARTBEAT_INTERVAL_SECS));

    sse.respond_to(&req)
}

/// GET /api/runs/:id/logs — read a single log file's content.
///
/// Query parameters:
/// - `file` — log file name (required)
#[get("/runs/{id}/logs")]
pub async fn get_log_file(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
    query: web::Query<LogFileQuery>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let run_id = path.into_inner();
    let file_name = query.file.as_deref().unwrap_or("default");

    // Validate file name
    let file_name = match sanitise_log_file_name(file_name) {
        Ok(name) => name,
        Err(reason) => {
            return ApiError::with_details(
                codes::UNKNOWN_LOG_FILE,
                reason,
                serde_json::json!({ "file": file_name }),
            )
            .error_response();
        }
    };

    // Look up the Run
    let run = match RunRepository::find_by_id(&pool, &run_id).await {
        Ok(r) => r,
        Err(_) => {
            return ApiError::with_details(
                codes::RUN_NOT_FOUND,
                "Run not found",
                serde_json::json!({ "id": run_id }),
            )
            .error_response();
        }
    };

    // RG ownership
    if let Err(e) = enforce_rg_ownership(&user, &run, &pool).await {
        return e.error_response();
    }

    // Resolve path
    let log_dir = match run.log_dir {
        Some(ref d) => PathBuf::from(d),
        None => {
            return ApiError::with_details(
                codes::RUN_NOT_FOUND,
                "Run has no log directory",
                serde_json::json!({ "run_id": run_id }),
            )
            .error_response();
        }
    };

    let log_path = match log_tailer::resolve_log_path(&log_dir, &file_name) {
        Some(p) => p,
        None => {
            return ApiError::with_details(
                codes::UNKNOWN_LOG_FILE,
                "Invalid log file name",
                serde_json::json!({ "file": file_name }),
            )
            .error_response();
        }
    };

    // Read file content
    match tokio::fs::read_to_string(&log_path).await {
        Ok(content) => HttpResponse::Ok()
            .content_type("text/plain; charset=utf-8")
            .body(content),
        Err(_) => {
            // File doesn't exist yet — return empty
            HttpResponse::Ok()
                .content_type("text/plain; charset=utf-8")
                .body("")
        }
    }
}

/// Query parameters for SSE log stream.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LogStreamQuery {
    pub file: Option<String>,
    pub level: Option<String>,
}

/// Query parameters for log file read.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LogFileQuery {
    pub file: Option<String>,
}

/// Enforce RG ownership: user must have access to the Run's Task's RG.
///
/// - Admin (resource_group_id = None): always allowed
/// - Non-admin with resource_group_id: must match the Task's RG
/// - Non-admin without resource_group_id (legacy): allowed (backward compat)
async fn enforce_rg_ownership(
    user: &UserContext,
    run: &Run,
    pool: &sqlx::SqlitePool,
) -> Result<(), ApiError> {
    // Admin always has access
    if user.role == "admin" {
        return Ok(());
    }

    // Check task.read permission
    rbac::require_action(user, RbacAction::TaskRead)?;

    // If the user has no resource_group_id (legacy or admin-like), allow access
    let user_rg = match &user.resource_group_id {
        Some(rg) => rg,
        None => return Ok(()),
    };

    // Look up the Task to check RG membership
    let task_id_ref = match run.task_id.as_ref() {
        Some(tid) => tid,
        None => {
            return Err(ApiError::with_details(
                codes::FORBIDDEN,
                "Run has no associated task (task was deleted)",
                serde_json::json!({ "run_id": run.id }),
            ));
        }
    };
    let task = match TaskRepository::find_by_id(pool, task_id_ref).await {
        Ok(t) => t,
        Err(_) => {
            return Err(ApiError::with_details(
                codes::FORBIDDEN,
                "Cannot determine resource group ownership",
                serde_json::json!({ "run_id": run.id, "task_id": task_id_ref }),
            ));
        }
    };

    // Check that the user's RG matches the task's RG
    if &task.resource_group_id != user_rg {
        return Err(ApiError::with_details(
            codes::FORBIDDEN,
            "Run belongs to a different resource group",
            serde_json::json!({ "run_id": run.id, "task_rg": task.resource_group_id, "user_rg": user_rg }),
        ));
    }

    Ok(())
}

/// Produce SSE events from a log file and send them through the channel.
async fn produce_sse_events(
    run_id: String,
    file_name: String,
    log_path: PathBuf,
    level_filter: Option<LogLevel>,
    last_event_id: Option<u64>,
    sse_state: LogSseState,
    event_tx: tokio::sync::mpsc::Sender<SseEvent>,
) {
    let poll_interval = Duration::from_millis(log_tailer::DEFAULT_POLL_INTERVAL_MS);

    // Emit replay-gap event if reconnecting after orchestrator restart
    if last_event_id.is_some() {
        let gap_event = SseEvent::Data(
            Data::new("Log stream restarted; events may have been missed since last connection")
                .event("replay-gap"),
        );
        if event_tx.send(gap_event).await.is_err() {
            return;
        }
    }

    // Create a stop channel for the tailer
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);

    // Create a broadcast channel for distributing chunks
    let (broadcast_tx, broadcast_rx) =
        tokio::sync::broadcast::channel::<LogChunkWithId>(EVENT_BUFFER_SIZE);

    // Register as a subscriber or start a new tailer
    let tailer_key = format!("{run_id}:{file_name}");
    let mut my_chunk_rx = {
        let mut tailers = sse_state.tailers.lock().await;
        if let Some(entry) = tailers.get_mut(&tailer_key) {
            // Existing tailer — just add a subscriber
            entry.subscriber_count += 1;
            broadcast_tx.subscribe()
        } else {
            // Start a new tailer
            let bcast_tx = broadcast_tx.clone();
            let tailer_entry = TailerEntry {
                stop_tx: stop_tx.clone(),
                subscriber_count: 1,
                chunk_tx: broadcast_tx.clone(),
            };
            tailers.insert(tailer_key.clone(), tailer_entry);

            // Spawn the tailer task
            let tail_run_id = run_id.clone();
            let tail_file_name = file_name.clone();
            let tail_log_path = log_path.clone();
            let tail_poll_interval = poll_interval;
            let tail_state = sse_state.clone();
            let tailer_key_clone = tailer_key.clone();

            tokio::spawn(async move {
                run_tailer_with_broadcast(
                    tail_run_id,
                    tail_file_name,
                    tail_log_path,
                    tail_poll_interval,
                    stop_rx,
                    bcast_tx,
                )
                .await;

                // Clean up the tailer entry when done
                let mut tailers = tail_state.tailers.lock().await;
                tailers.remove(&tailer_key_clone);
            });

            broadcast_rx
        }
    };

    // Rate limiter for this subscriber
    let mut rate_limit = SubscriberRateLimit::new(DEFAULT_RATE_LIMIT_PER_SEC);
    let mut event_id: u64 = last_event_id.unwrap_or(0);
    let mut pending_lines: Vec<String> = Vec::new();

    loop {
        // Check for new chunks with timeout for heartbeat
        match tokio::time::timeout(
            Duration::from_secs(DEFAULT_HEARTBEAT_INTERVAL_SECS),
            my_chunk_rx.recv(),
        )
        .await
        {
            Ok(Ok(chunk)) => {
                // Apply level filter per-line: only buffer lines that match
                let filtered: Vec<&String> = match level_filter {
                    Some(ref level) => chunk
                        .lines
                        .iter()
                        .filter(|l| level.matches_line(l))
                        .collect(),
                    None => chunk.lines.iter().collect(),
                };

                if filtered.is_empty() {
                    continue;
                }

                // Buffer only matching lines for potential coalescing
                for line in filtered {
                    pending_lines.push(line.to_string());
                }

                // Try to emit
                if rate_limit.check() {
                    event_id += 1;
                    let data = pending_lines.join("\n");
                    let event =
                        SseEvent::Data(Data::new(data).id(event_id.to_string()).event("log"));
                    if event_tx.send(event).await.is_err() {
                        break; // Client disconnected
                    }
                    pending_lines.clear();
                }
                // If rate-limited, lines stay buffered for next emission
                // (burst coalescing)
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                // We missed some events — emit replay-gap
                event_id += 1;
                let gap_event = SseEvent::Data(
                    Data::new(format!("Missed {n} events due to buffer overflow"))
                        .id(event_id.to_string())
                        .event("replay-gap"),
                );
                if event_tx.send(gap_event).await.is_err() {
                    break;
                }
            }
            Ok(Err(_)) => {
                // Channel closed — tailer stopped
                break;
            }
            Err(_) => {
                // Timeout — emit heartbeat comment
                let heartbeat = SseEvent::Comment("heartbeat".into());
                if event_tx.send(heartbeat).await.is_err() {
                    break;
                }
            }
        }

        // If we have pending lines and the rate window has reset, flush them
        if !pending_lines.is_empty() && rate_limit.check() {
            event_id += 1;
            let data = pending_lines.join("\n");
            let event = SseEvent::Data(Data::new(data).id(event_id.to_string()).event("log"));
            if event_tx.send(event).await.is_err() {
                break;
            }
            pending_lines.clear();
        }
    }

    // Clean up: decrement subscriber count or stop tailer
    {
        let mut tailers = sse_state.tailers.lock().await;
        if let Some(entry) = tailers.get_mut(&tailer_key) {
            entry.subscriber_count = entry.subscriber_count.saturating_sub(1);
            if entry.subscriber_count == 0 {
                let _ = entry.stop_tx.send(true);
            }
        }
    }
}

/// Run a LogTailer and broadcast chunks to all subscribers.
async fn run_tailer_with_broadcast(
    run_id: String,
    file_name: String,
    log_path: PathBuf,
    poll_interval: Duration,
    stop_rx: tokio::sync::watch::Receiver<bool>,
    broadcast_tx: tokio::sync::broadcast::Sender<LogChunkWithId>,
) {
    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<log_tailer::LogChunk>(100);

    // Spawn the actual file tailer
    let tailer_handle = {
        let path = log_path.clone();
        let name = file_name.clone();
        tokio::spawn(async move {
            log_tailer::tail_file(&name, path, 0, poll_interval, stop_rx.clone(), chunk_tx).await;
        })
    };

    // Forward chunks to broadcast
    let mut event_id: u64 = 0;
    while let Some(chunk) = chunk_rx.recv().await {
        event_id += 1;
        let with_id = LogChunkWithId {
            event_id,
            file: chunk.file,
            lines: vec![chunk.line],
        };
        // Broadcast ignores send errors (no subscribers)
        let _ = broadcast_tx.send(with_id);
    }

    let _ = tailer_handle.await;
    tracing::debug!("tailer for {run_id}:{file_name} stopped");
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscriber_rate_limit_allows_under_cap() {
        let mut rl = SubscriberRateLimit::new(5);
        for _ in 0..5 {
            assert!(rl.check());
        }
    }

    #[test]
    fn test_subscriber_rate_limit_blocks_over_cap() {
        let mut rl = SubscriberRateLimit::new(3);
        assert!(rl.check());
        assert!(rl.check());
        assert!(rl.check());
        assert!(!rl.check()); // Over cap
    }

    #[test]
    fn test_subscriber_rate_limit_resets_after_window() {
        let mut rl = SubscriberRateLimit::new(2);
        assert!(rl.check());
        assert!(rl.check());
        assert!(!rl.check()); // Over cap

        // Simulate window reset by advancing the window start
        rl.window_start = std::time::Instant::now() - Duration::from_secs(2);
        assert!(rl.check()); // Should reset and allow
    }

    #[test]
    fn test_subscriber_rate_limit_independent_per_instance() {
        let mut rl1 = SubscriberRateLimit::new(2);
        let mut rl2 = SubscriberRateLimit::new(2);

        // Exhaust rl1
        assert!(rl1.check());
        assert!(rl1.check());
        assert!(!rl1.check()); // rl1 is over cap

        // rl2 is still independent
        assert!(rl2.check());
        assert!(rl2.check());
        assert!(!rl2.check()); // rl2 is now also over cap
    }

    #[test]
    fn test_sanitise_rejects_url_encoded_traversal() {
        assert!(sanitise_log_file_name("%2E%2E%2Fetc%2Fpasswd").is_err());
        assert!(sanitise_log_file_name("%2E%2E").is_err());
    }

    #[test]
    fn test_sse_event_data_construction() {
        let event = SseEvent::Data(Data::new("hello").id("1").event("log"));
        // Verify construction succeeds
        assert!(matches!(event, SseEvent::Data(_)));
    }

    #[test]
    fn test_sse_event_comment_construction() {
        let event: SseEvent = SseEvent::Comment("heartbeat".into());
        assert!(matches!(event, SseEvent::Comment(_)));
    }

    /// Verify that per-line level filtering only forwards matching lines,
    /// NOT all lines when any line matches (the old any()-on-chunk bug).
    #[test]
    fn test_log_level_filter_per_line_not_per_batch() {
        let error_level = LogLevel::Error;

        // A chunk with mixed lines: one ERROR, one INFO, one WARN
        let lines = [
            "[ERROR] something broke".to_string(),
            "[INFO] all is fine".to_string(),
            "[WARN] be careful".to_string(),
        ];

        // Per-line filtering: only ERROR line matches
        let filtered: Vec<&String> = lines
            .iter()
            .filter(|l| error_level.matches_line(l))
            .collect();
        assert_eq!(filtered.len(), 1, "only ERROR lines should pass the filter");
        assert_eq!(filtered[0], "[ERROR] something broke");

        // Verify the old any()-on-chunk behaviour would have been wrong:
        // any() would return true (ERROR matches), causing ALL lines to be forwarded.
        let any_match = lines.iter().any(|l| error_level.matches_line(l));
        assert!(
            any_match,
            "any() would return true — old bug would forward all lines"
        );
        // But with per-line filter, non-matching lines are excluded
        assert!(!error_level.matches_line(&lines[1]));
        assert!(!error_level.matches_line(&lines[2]));
    }
}
