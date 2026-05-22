//! SSE session tracker — tracks active SSE connections per session token
//! and closes them when a session is invalidated (logout, expiry, disable).
//!
//! When a client subscribes to an SSE stream (log or alert), the session token
//! is registered here with a cancellation token. When the session is invalidated,
//! `close_all_for_session` signals all cancellation tokens, which causes the
//! SSE producer tasks to stop and close the streams.
//!
//! Previous implementation used mpsc sender clones, but the producer task
//! held its own sender clone, so dropping the tracker's clone didn't close
//! the channel. Using a watch-based cancellation signal ensures the producer
//! is notified to stop.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Cancellation handle for a single SSE connection.
///
/// When cancelled, the producer task should stop sending events and
/// close the SSE stream.
#[derive(Debug, Clone)]
pub struct SseCancelHandle {
    /// Watch sender: sending `true` means "cancelled".
    cancel_tx: tokio::sync::watch::Sender<bool>,
    /// Watch receiver for the producer to check.
    cancel_rx: tokio::sync::watch::Receiver<bool>,
}

impl SseCancelHandle {
    /// Create a new un-cancelled handle pair.
    pub fn new() -> Self {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        Self {
            cancel_tx,
            cancel_rx,
        }
    }
}

impl Default for SseCancelHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl SseCancelHandle {
    /// Signal cancellation. The producer should stop after this.
    pub fn cancel(&self) {
        let _ = self.cancel_tx.send(true);
    }

    /// Get a receiver clone that the producer can check for cancellation.
    pub fn receiver(&self) -> tokio::sync::watch::Receiver<bool> {
        self.cancel_rx.clone()
    }

    /// Check if cancellation has been signalled.
    pub fn is_cancelled(&self) -> bool {
        *self.cancel_rx.borrow()
    }
}

/// Shared state for tracking SSE connections per session.
#[derive(Debug, Clone, Default)]
pub struct SseSessionTracker {
    /// Map: session token → list of cancellation handles for active SSE connections.
    connections: Arc<Mutex<HashMap<String, Vec<SseCancelHandle>>>>,
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
    /// Returns a `SseCancelHandle` whose receiver should be checked by the
    /// SSE producer task. When the session is invalidated, `cancel()` will
    /// be called on the handle, signalling the producer to stop.
    pub async fn register(&self, session_token: &str) -> SseCancelHandle {
        let handle = SseCancelHandle::new();
        let mut conns = self.connections.lock().await;
        conns
            .entry(session_token.to_string())
            .or_default()
            .push(handle.clone());
        handle
    }

    /// Unregister an SSE connection for a session.
    ///
    /// Called when the SSE stream ends naturally (client disconnect).
    /// Removes one cancellation handle from the tracker.
    pub async fn unregister(&self, session_token: &str) {
        let mut conns = self.connections.lock().await;
        if let Some(handles) = conns.get_mut(session_token) {
            handles.pop();
            if handles.is_empty() {
                conns.remove(session_token);
            }
        }
    }

    /// Close all SSE connections for a session.
    ///
    /// Called when a session is invalidated (logout, expiry, disable).
    /// Signals all cancellation handles, which causes the SSE producer
    /// tasks to stop and close the streams.
    /// Returns the number of connections that were closed.
    pub async fn close_all_for_session(&self, session_token: &str) -> usize {
        let mut conns = self.connections.lock().await;
        conns
            .remove(session_token)
            .map(|handles| {
                let count = handles.len();
                for handle in &handles {
                    handle.cancel();
                }
                count
            })
            .unwrap_or(0)
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

        // Register two connections for session "abc"
        let h1 = tracker.register("abc").await;
        let h2 = tracker.register("abc").await;
        assert_eq!(tracker.connection_count("abc").await, 2);
        assert!(!h1.is_cancelled());
        assert!(!h2.is_cancelled());

        // Close all for session "abc"
        let closed = tracker.close_all_for_session("abc").await;
        assert_eq!(closed, 2);
        assert_eq!(tracker.connection_count("abc").await, 0);
        assert!(h1.is_cancelled());
        assert!(h2.is_cancelled());
    }

    #[tokio::test]
    async fn test_unregister_removes_one() {
        let tracker = SseSessionTracker::new();

        let _h1 = tracker.register("abc").await;
        let _h2 = tracker.register("abc").await;
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
    async fn test_cancel_handle_signals_producer() {
        let tracker = SseSessionTracker::new();
        let handle = tracker.register("session1").await;

        // The producer gets a receiver clone
        let mut rx = handle.receiver();
        assert!(!*rx.borrow_and_update());

        // Close all — this signals cancellation
        let closed = tracker.close_all_for_session("session1").await;
        assert_eq!(closed, 1);

        // The receiver should now see true
        assert!(rx.changed().await.is_ok());
        assert!(*rx.borrow());
    }

    #[tokio::test]
    async fn test_multiple_sessions_independent() {
        let tracker = SseSessionTracker::new();
        let h_a = tracker.register("session_a").await;
        let h_b = tracker.register("session_b").await;

        // Close session_a — session_b should remain
        let closed = tracker.close_all_for_session("session_a").await;
        assert_eq!(closed, 1);
        assert!(h_a.is_cancelled());
        assert!(!h_b.is_cancelled());
        assert_eq!(tracker.connection_count("session_b").await, 1);
        assert_eq!(tracker.total_connections().await, 1);
    }

    #[tokio::test]
    async fn test_cancel_handle_standalone() {
        let handle = SseCancelHandle::new();
        let mut rx = handle.receiver();
        assert!(!handle.is_cancelled());

        handle.cancel();
        assert!(handle.is_cancelled());

        // Receiver sees the change
        assert!(rx.changed().await.is_ok());
        assert!(*rx.borrow());
    }
}
