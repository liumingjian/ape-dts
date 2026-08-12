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
//!
//! Keys are **namespaced** as `user_id:method:path:key` (see
//! [`extract_scoped_key`]). A bare header value would be shared across every
//! endpoint, so a client that reuses one key for a "restart" — stop, then
//! start — would get the stop's cached 202 replayed for the start, and the
//! task would never actually start while the API reported success.

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

/// How often the background evictor sweeps expired entries.
const EVICTION_INTERVAL: Duration = Duration::from_secs(60);

/// Spawn the background evictor for `cache`.
///
/// `get` only evicts the key it is asked about, so without this sweep the map
/// grows for the process's whole lifetime — every key ever seen stays resident
/// long after its TTL.
pub fn spawn_evictor(cache: IdempotencyCache) -> tokio::task::JoinHandle<()> {
    spawn_evictor_every(cache, EVICTION_INTERVAL)
}

/// Spawn the background evictor with an explicit sweep interval (for tests).
pub fn spawn_evictor_every(
    cache: IdempotencyCache,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // The first tick fires immediately; skip it, nothing can be stale yet.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            cache.evict_expired().await;
        }
    })
}

/// Header name for the idempotency key.
pub const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";

/// Build the namespaced cache key for an idempotency key.
///
/// The namespace is `user_id:method:path:key`, so the same header value
/// reused across users, verbs, or routes never collides. `path` carries the
/// resource id, so start-task-A and start-task-B are distinct too.
pub fn scoped_key(user_id: &str, method: &str, path: &str, key: &str) -> String {
    format!("{user_id}:{method}:{path}:{key}")
}

/// Extract the Idempotency-Key from a request and namespace it for `user_id`.
///
/// Handlers should use this rather than [`extract_key`]: it is the only form
/// that is safe to hand to [`IdempotencyCache`].
pub fn extract_scoped_key(req: &actix_web::HttpRequest, user_id: &str) -> Option<String> {
    extract_key(req).map(|key| scoped_key(user_id, req.method().as_str(), req.path(), &key))
}

/// Extract the raw Idempotency-Key from a request, if present.
///
/// The raw value is *not* a cache key — namespace it with [`scoped_key`]
/// (or use [`extract_scoped_key`]) before touching the cache.
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
    fn test_scoped_key_namespaces_user_method_and_path() {
        assert_eq!(
            scoped_key("u1", "POST", "/api/tasks/t1/start", "k"),
            "u1:POST:/api/tasks/t1/start:k"
        );
    }

    #[tokio::test]
    async fn test_same_raw_key_on_start_and_stop_does_not_collide() {
        // The bug this namespacing fixes: a client reusing one key for
        // "restart" (stop then start) used to get the stop's cached response
        // replayed for the start, so the start never ran.
        let cache = IdempotencyCache::new();
        let stop = scoped_key("u1", "POST", "/api/tasks/t1/stop", "restart-1");
        let start = scoped_key("u1", "POST", "/api/tasks/t1/start", "restart-1");

        cache
            .put(&stop, 202, serde_json::json!({"run_id": "old"}))
            .await;

        assert!(
            cache.get(&start).await.is_none(),
            "start must not read the stop's cached response"
        );
    }

    #[tokio::test]
    async fn test_same_raw_key_across_users_does_not_collide() {
        let cache = IdempotencyCache::new();
        let a = scoped_key("user-a", "POST", "/api/tasks/t1/start", "k");
        let b = scoped_key("user-b", "POST", "/api/tasks/t1/start", "k");
        cache.put(&a, 202, serde_json::json!({"run_id": "a"})).await;
        assert!(cache.get(&b).await.is_none());
    }

    #[tokio::test]
    async fn test_same_raw_key_across_tasks_does_not_collide() {
        let cache = IdempotencyCache::new();
        let t1 = scoped_key("u1", "POST", "/api/tasks/t1/start", "k");
        let t2 = scoped_key("u1", "POST", "/api/tasks/t2/start", "k");
        cache
            .put(&t1, 202, serde_json::json!({"run_id": "r1"}))
            .await;
        assert!(cache.get(&t2).await.is_none());
    }

    #[test]
    fn test_extract_scoped_key_uses_request_method_and_path() {
        let req = actix_web::test::TestRequest::post()
            .uri("/api/tasks/t1/start")
            .insert_header(("Idempotency-Key", "abc-123"))
            .to_http_request();
        assert_eq!(
            extract_scoped_key(&req, "u1"),
            Some("u1:POST:/api/tasks/t1/start:abc-123".to_string())
        );
    }

    #[test]
    fn test_extract_scoped_key_absent_stays_none() {
        let req = actix_web::test::TestRequest::post()
            .uri("/api/tasks/t1/start")
            .to_http_request();
        assert!(extract_scoped_key(&req, "u1").is_none());
    }

    #[tokio::test]
    async fn test_spawn_evictor_sweeps_expired_entries() {
        let cache = IdempotencyCache::new();
        // Insert an already-expired entry: the sweep, not the TTL, is the
        // thing under test, so the interval is what we shorten.
        {
            let mut map = cache.inner.lock().await;
            map.insert(
                "k".to_string(),
                CachedResponse {
                    status: 202,
                    body: serde_json::json!({"run_id": "r"}),
                    cached_at: Instant::now() - Duration::from_secs(120),
                },
            );
        }
        let handle = spawn_evictor_every(cache.clone(), Duration::from_millis(20));

        // Two ticks' worth: the first tick is consumed at startup.
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            cache.inner.lock().await.is_empty(),
            "the evictor must drop expired entries without anyone reading them"
        );
        handle.abort();
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
