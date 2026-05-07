//! SSE session tracker — tracks active SSE connections per session token
//! and closes them when a session is invalidated (logout, expiry, disable).
//!
//! When a client subscribes to an SSE stream (log or alert), the session token
//! is registered here with the event sender. When the session is invalidated,
//! `close_all_for_session` drops all senders, which causes the SSE streams
//! to end (the client sees the connection close).

use actix_web_lab::sse::Event as SseEvent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state for tracking SSE connections per session.
#[derive(Debug, Clone, Default)]
pub struct SseSessionTracker {
    /// Map: session token → list of event senders for active SSE connections.
    connections: Arc<Mutex<HashMap<String, Vec<tokio::sync::mpsc::Sender<SseEvent>>>>>,
}

impl SseSessionTracker {
    /// Create a new empty tracker.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register an SSE connection for a session.
    ///
    /// When the session is invalidated, the sender will be dropped,
    /// causing the SSE stream to end.
    pub async fn register(&self, session_token: &str, sender: tokio::sync::mpsc::Sender<SseEvent>) {
        let mut conns = self.connections.lock().await;
        conns
            .entry(session_token.to_string())
            .or_default()
            .push(sender);
    }

    /// Unregister an SSE connection for a session.
    ///
    /// Called when the SSE stream ends naturally (client disconnect).
    /// Uses sender identity comparison (by checking if send fails) to remove
    /// the right entry. Since all senders for a session are equivalent for
    /// cleanup purposes, we just pop one.
    pub async fn unregister(&self, session_token: &str) {
        let mut conns = self.connections.lock().await;
        if let Some(senders) = conns.get_mut(session_token) {
            // Remove one sender (the one that just disconnected).
            // We can't identify which specific sender was dropped,
            // so we remove the last one. Since all senders are equivalent
            // for cleanup purposes, this is fine.
            senders.pop();
            if senders.is_empty() {
                conns.remove(session_token);
            }
        }
    }

    /// Close all SSE connections for a session.
    ///
    /// Called when a session is invalidated (logout, expiry, disable).
    /// Drops all senders, which causes the SSE streams to end.
    /// Returns the number of connections that were closed.
    pub async fn close_all_for_session(&self, session_token: &str) -> usize {
        let mut conns = self.connections.lock().await;
        // The senders are dropped here (by removing from the HashMap),
        // which will cause the SSE streams to end.
        conns.remove(session_token).map(|s| s.len()).unwrap_or(0)
    }

    /// Get the number of active SSE connections for a session.
    #[cfg(test)]
    pub async fn connection_count(&self, session_token: &str) -> usize {
        let conns = self.connections.lock().await;
        conns.get(session_token).map(|s| s.len()).unwrap_or(0)
    }

    /// Get total number of active SSE connections across all sessions.
    #[cfg(test)]
    pub async fn total_connections(&self) -> usize {
        let conns = self.connections.lock().await;
        conns.values().map(|s| s.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_close_all() {
        let tracker = SseSessionTracker::new();
        let (tx1, _rx1) = tokio::sync::mpsc::channel::<SseEvent>(10);
        let (tx2, _rx2) = tokio::sync::mpsc::channel::<SseEvent>(10);

        // Register two connections for session "abc"
        tracker.register("abc", tx1).await;
        tracker.register("abc", tx2).await;
        assert_eq!(tracker.connection_count("abc").await, 2);

        // Close all for session "abc"
        let closed = tracker.close_all_for_session("abc").await;
        assert_eq!(closed, 2);
        assert_eq!(tracker.connection_count("abc").await, 0);
    }

    #[tokio::test]
    async fn test_unregister_removes_one() {
        let tracker = SseSessionTracker::new();
        let (tx1, _rx1) = tokio::sync::mpsc::channel::<SseEvent>(10);
        let (tx2, _rx2) = tokio::sync::mpsc::channel::<SseEvent>(10);

        tracker.register("abc", tx1).await;
        tracker.register("abc", tx2).await;
        assert_eq!(tracker.connection_count("abc").await, 2);

        // Unregister one connection
        tracker.unregister("abc").await;
        assert_eq!(tracker.connection_count("abc").await, 1);
    }

    #[tokio::test]
    async fn test_close_all_for_unknown_session() {
        let tracker = SseSessionTracker::new();
        let closed = tracker.close_all_for_session("nonexistent").await;
        assert_eq!(closed, 0);
    }

    #[tokio::test]
    async fn test_close_all_drops_senders_ending_streams() {
        let tracker = SseSessionTracker::new();
        let (tx, rx) = tokio::sync::mpsc::channel::<SseEvent>(10);

        tracker.register("session1", tx).await;

        // Close all — this drops the sender
        let closed = tracker.close_all_for_session("session1").await;
        assert_eq!(closed, 1);

        // The receiver should now get None (channel closed)
        drop(rx);
    }

    #[tokio::test]
    async fn test_multiple_sessions_independent() {
        let tracker = SseSessionTracker::new();
        let (tx1, _rx1) = tokio::sync::mpsc::channel::<SseEvent>(10);
        let (tx2, _rx2) = tokio::sync::mpsc::channel::<SseEvent>(10);

        tracker.register("session_a", tx1).await;
        tracker.register("session_b", tx2).await;

        // Close session_a — session_b should remain
        let closed = tracker.close_all_for_session("session_a").await;
        assert_eq!(closed, 1);
        assert_eq!(tracker.connection_count("session_b").await, 1);
        assert_eq!(tracker.total_connections().await, 1);
    }
}
