//! PortPool — thread-safe per-Run metrics port allocator.
//!
//! Allocates ports from [PORT_MIN, PORT_MAX] for use as the engine's
//! Prometheus exposition port. The range explicitly excludes 9090 (the old
//! hardcoded default) because the allocator only covers 9100–9199.
//!
//! On orchestrator startup, `seed` must be called with the set of ports
//! already held by running Runs (read from `runs.metrics_port` in the DB)
//! so that a restart never double-allocates a port.

use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Inclusive lower bound of the metrics port range.
pub const PORT_MIN: u16 = 9100;
/// Inclusive upper bound of the metrics port range.
pub const PORT_MAX: u16 = 9199;

/// Thread-safe metrics port allocator for the [PORT_MIN, PORT_MAX] range.
#[derive(Clone, Default)]
pub struct PortPool {
    in_use: Arc<Mutex<BTreeSet<u16>>>,
}

impl PortPool {
    pub fn new() -> Self {
        Self {
            in_use: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    /// Mark a set of ports as already in use (called once on orchestrator startup).
    ///
    /// Ports outside [PORT_MIN, PORT_MAX] are silently ignored.
    pub async fn seed(&self, ports: impl IntoIterator<Item = u16>) {
        let mut guard = self.in_use.lock().await;
        for p in ports {
            if p >= PORT_MIN && p <= PORT_MAX {
                guard.insert(p);
            }
        }
    }

    /// Allocate the lowest available port in [PORT_MIN, PORT_MAX].
    ///
    /// Returns `None` when all 100 ports are in use (pool exhausted).
    pub async fn acquire(&self) -> Option<u16> {
        let mut guard = self.in_use.lock().await;
        for port in PORT_MIN..=PORT_MAX {
            if !guard.contains(&port) {
                guard.insert(port);
                return Some(port);
            }
        }
        None
    }

    /// Return a port to the pool so it can be reused by future Runs.
    ///
    /// No-ops if the port is not currently marked in-use.
    pub async fn release(&self, port: u16) {
        let mut guard = self.in_use.lock().await;
        guard.remove(&port);
    }

    /// Return the number of ports currently in use (test helper).
    #[cfg(test)]
    pub async fn in_use_count(&self) -> usize {
        self.in_use.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acquire_returns_port_in_range() {
        let pool = PortPool::new();
        let port = pool.acquire().await.expect("should get a port");
        assert!(
            port >= PORT_MIN && port <= PORT_MAX,
            "port {port} not in range"
        );
    }

    #[tokio::test]
    async fn acquire_never_returns_9090() {
        let pool = PortPool::new();
        // Exhaust the entire pool and verify 9090 is never returned.
        let mut ports = Vec::new();
        while let Some(p) = pool.acquire().await {
            assert_ne!(p, 9090, "port 9090 must never be returned");
            ports.push(p);
        }
        assert_eq!(ports.len(), 100, "pool should have exactly 100 ports");
    }

    #[tokio::test]
    async fn sequential_acquires_return_increasing_ports() {
        let pool = PortPool::new();
        let p1 = pool.acquire().await.unwrap();
        let p2 = pool.acquire().await.unwrap();
        let p3 = pool.acquire().await.unwrap();
        assert_eq!(p1, PORT_MIN);
        assert_eq!(p2, PORT_MIN + 1);
        assert_eq!(p3, PORT_MIN + 2);
    }

    #[tokio::test]
    async fn release_makes_port_reacquirable() {
        let pool = PortPool::new();
        let port = pool.acquire().await.unwrap();
        assert_eq!(pool.in_use_count().await, 1);
        pool.release(port).await;
        assert_eq!(pool.in_use_count().await, 0);
        let port2 = pool.acquire().await.unwrap();
        assert_eq!(port, port2, "released port should be reused as the lowest");
    }

    #[tokio::test]
    async fn exhaustion_returns_none() {
        let pool = PortPool::new();
        // Fill all 100 slots.
        for _ in 0..100 {
            assert!(pool.acquire().await.is_some());
        }
        // Next acquire must fail.
        assert!(pool.acquire().await.is_none());
    }

    #[tokio::test]
    async fn concurrent_acquires_return_distinct_ports() {
        let pool = PortPool::new();
        let pool = Arc::new(pool);

        let mut handles = Vec::new();
        for _ in 0..50 {
            let p = pool.clone();
            handles.push(tokio::spawn(async move { p.acquire().await }));
        }

        let mut ports = Vec::new();
        for h in handles {
            let port = h.await.unwrap().expect("should get a port");
            ports.push(port);
        }

        // All 50 ports must be distinct.
        let unique: std::collections::HashSet<u16> = ports.iter().copied().collect();
        assert_eq!(
            unique.len(),
            50,
            "all concurrent acquires must return distinct ports"
        );

        // All ports must be in range.
        for &p in &ports {
            assert!(p >= PORT_MIN && p <= PORT_MAX);
        }
    }

    #[tokio::test]
    async fn seed_prevents_acquiring_seeded_ports() {
        let pool = PortPool::new();
        // Seed ports 9100 and 9101 as in-use.
        pool.seed([9100u16, 9101u16]).await;
        let port = pool.acquire().await.unwrap();
        // Should skip 9100 and 9101 and return 9102.
        assert_eq!(port, 9102);
    }

    #[tokio::test]
    async fn seed_ignores_out_of_range_ports() {
        let pool = PortPool::new();
        // Seed 9090 (below range) and 9200 (above range).
        pool.seed([9090u16, 9200u16]).await;
        // Pool should still have all 100 slots available.
        assert_eq!(pool.in_use_count().await, 0);
        let port = pool.acquire().await.unwrap();
        assert_eq!(port, PORT_MIN);
    }
}
