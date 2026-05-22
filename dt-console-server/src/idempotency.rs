//! Idempotency-Key dedup cache for lifecycle and clear endpoints.
//!
//! Per VAL-CONS-IDEMPOTENCY-001: when a request includes an `Idempotency-Key`
//! header, the server checks if that key has been seen within the TTL window.
//! If so, the cached response is returned. Otherwise, the request executes
//! normally and the result is cached with a 60-second TTL.
//!
//! The cache is in-memory and per-process. Keys expire after 60 seconds.
//! This is sufficient to deduplicate retries from network blips or
//! client-side retries.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};

/// How long a cached idempotency entry remains valid.
const IDEMPOTENCY_TTL: Duration = Duration::from_secs(60);

/// A cached response for an idempotency key.
#[derive(Debug, Clone)]
pub struct CachedResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The JSON body (serialized).
    pub body: serde_json::Value,
    /// When this entry was cached.
    pub cached_at: Instant,
}

/// Thread-safe idempotency cache keyed by Idempotency-Key header value.
#[derive(Debug, Clone, Default)]
pub struct IdempotencyCache {
    inner: Arc<Mutex<HashMap<String, CachedResponse>>>,
}

impl IdempotencyCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Look up a cached response by key.
    ///
    /// Returns `Some(cached_response)` if the key exists and has not expired,
    /// `None` otherwise. Expired entries are evicted on lookup.
    pub async fn get(&self, key: &str) -> Option<CachedResponse> {
        let mut map = self.inner.lock().await;
        if let Some(cached) = map.get(key) {
            if cached.cached_at.elapsed() < IDEMPOTENCY_TTL {
                return Some(cached.clone());
            }
            // Expired — evict.
            map.remove(key);
        }
        None
    }

    /// Store a response under the given key.
    ///
    /// If the key already exists (and is unexpired), the existing entry is
    /// preserved (no overwrite). This is safe because concurrent requests
    /// with the same key should get the same result.
    pub async fn put(&self, key: &str, status: u16, body: serde_json::Value) {
        let mut map = self.inner.lock().await;
        // Don't overwrite an existing unexpired entry.
        if let Some(existing) = map.get(key) {
            if existing.cached_at.elapsed() < IDEMPOTENCY_TTL {
                return;
            }
        }
        map.insert(
            key.to_string(),
            CachedResponse {
                status,
                body,
                cached_at: Instant::now(),
            },
        );
    }

    /// Evict all expired entries. Called opportunistically.
    pub async fn evict_expired(&self) {
        let mut map = self.inner.lock().await;
        map.retain(|_, v| v.cached_at.elapsed() < IDEMPOTENCY_TTL);
    }
}

/// Header name for the idempotency key.
pub const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";

/// Extract the Idempotency-Key from a request, if present.
pub fn extract_key(req: &actix_web::HttpRequest) -> Option<String> {
    req.headers()
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_put_and_get() {
        let cache = IdempotencyCache::new();
        cache
            .put("key1", 202, serde_json::json!({"run_id": "abc"}))
            .await;

        let result = cache.get("key1").await;
        assert!(result.is_some());
        let cached = result.unwrap();
        assert_eq!(cached.status, 202);
        assert_eq!(cached.body["run_id"], "abc");
    }

    #[tokio::test]
    async fn test_cache_miss_returns_none() {
        let cache = IdempotencyCache::new();
        let result = cache.get("nonexistent").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_expired_entry_evicted_on_get() {
        let cache = IdempotencyCache::new();
        // Manually insert an entry with an expired timestamp.
        {
            let mut map = cache.inner.lock().await;
            map.insert(
                "expired-key".to_string(),
                CachedResponse {
                    status: 202,
                    body: serde_json::json!({"run_id": "old"}),
                    cached_at: Instant::now() - Duration::from_secs(120),
                },
            );
        }

        let result = cache.get("expired-key").await;
        assert!(result.is_none(), "expired entry should be evicted");

        // Verify it was actually removed from the map.
        let map = cache.inner.lock().await;
        assert!(!map.contains_key("expired-key"));
    }

    #[tokio::test]
    async fn test_cache_put_does_not_overwrite_unexpired() {
        let cache = IdempotencyCache::new();
        cache
            .put("key1", 202, serde_json::json!({"run_id": "first"}))
            .await;

        // Second put with the same key should not overwrite.
        cache
            .put("key1", 409, serde_json::json!({"code": "CONFLICT"}))
            .await;

        let result = cache.get("key1").await.unwrap();
        assert_eq!(result.status, 202);
        assert_eq!(result.body["run_id"], "first");
    }

    #[tokio::test]
    async fn test_evict_expired_removes_old_entries() {
        let cache = IdempotencyCache::new();
        cache
            .put("fresh-key", 202, serde_json::json!({"run_id": "fresh"}))
            .await;

        // Manually insert an expired entry.
        {
            let mut map = cache.inner.lock().await;
            map.insert(
                "old-key".to_string(),
                CachedResponse {
                    status: 202,
                    body: serde_json::json!({"run_id": "old"}),
                    cached_at: Instant::now() - Duration::from_secs(120),
                },
            );
        }

        cache.evict_expired().await;

        // Fresh key should still exist.
        assert!(cache.get("fresh-key").await.is_some());
        // Old key should be gone.
        assert!(cache.get("old-key").await.is_none());
    }

    #[test]
    fn test_extract_key_present() {
        let req = actix_web::test::TestRequest::default()
            .insert_header(("Idempotency-Key", "abc-123"))
            .to_http_request();
        let key = extract_key(&req);
        assert_eq!(key, Some("abc-123".to_string()));
    }

    #[test]
    fn test_extract_key_absent() {
        let req = actix_web::test::TestRequest::default().to_http_request();
        let key = extract_key(&req);
        assert!(key.is_none());
    }

    #[test]
    fn test_extract_key_empty_ignored() {
        let req = actix_web::test::TestRequest::default()
            .insert_header(("Idempotency-Key", "  "))
            .to_http_request();
        let key = extract_key(&req);
        assert!(key.is_none());
    }
}
