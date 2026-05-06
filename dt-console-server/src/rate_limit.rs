//! In-memory rate limiter for login attempts.
//!
//! Tracks failed login attempts per (username, IP) pair. After `max_attempts`
//! failures within `window_secs`, subsequent attempts are rejected with 429
//! for the remainder of the window.

use crate::error::{codes, ApiError};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
struct AttemptRecord {
    count: u32,
    window_start: Instant,
}

/// Configuration for the rate limiter.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum failed attempts before rate-limiting kicks in.
    pub max_attempts: u32,
    /// Window in seconds for counting failed attempts.
    pub window_secs: u64,
    /// Duration in seconds the client is blocked after exceeding the limit.
    pub block_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            window_secs: 60,
            block_secs: 60,
        }
    }
}

/// Thread-safe in-memory rate limiter using DashMap.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    attempts: Arc<DashMap<String, AttemptRecord>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            attempts: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Check if the given (username, ip) is rate-limited, and record a failed
    /// attempt. Returns `Ok(())` if the request is allowed, or an `ApiError`
    /// with 429 if rate-limited.
    ///
    /// Call this **before** checking credentials. If the credentials are
    /// actually valid, call [`Self::clear`] to remove the rate-limit record.
    pub fn check_and_record(&self, username: &str, ip: &str) -> Result<(), ApiError> {
        let key = format!("{username}:{ip}");
        let now = Instant::now();

        let mut entry = self.attempts.entry(key).or_insert(AttemptRecord {
            count: 0,
            window_start: now,
        });

        // Reset window if expired
        if now.duration_since(entry.window_start).as_secs()
            > self.config.window_secs + self.config.block_secs
        {
            entry.count = 0;
            entry.window_start = now;
        }

        // Check if currently blocked
        let elapsed = now.duration_since(entry.window_start).as_secs();
        if entry.count >= self.config.max_attempts {
            // Still within block period?
            if elapsed < self.config.window_secs + self.config.block_secs {
                let remaining = (self.config.window_secs + self.config.block_secs - elapsed) as u32;
                return Err(ApiError::with_details(
                    codes::TOO_MANY_ATTEMPTS,
                    "Too many failed login attempts",
                    serde_json::json!({ "retry_after_secs": remaining }),
                ));
            }
            // Block period expired, reset
            entry.count = 0;
            entry.window_start = now;
        }

        // Record the failed attempt
        entry.count += 1;
        Ok(())
    }

    /// Clear rate-limit records for the given (username, ip) after a
    /// successful login. This prevents a prior failed attempt from
    /// counting toward the rate limit after a valid login.
    pub fn clear(&self, username: &str, ip: &str) {
        let key = format!("{username}:{ip}");
        self.attempts.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_attempts: 3,
            window_secs: 60,
            block_secs: 60,
        });

        for _ in 0..3 {
            assert!(limiter.check_and_record("alice", "1.2.3.4").is_ok());
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_attempts: 3,
            window_secs: 60,
            block_secs: 60,
        });

        for _ in 0..3 {
            limiter.check_and_record("bob", "5.6.7.8").unwrap();
        }
        let result = limiter.check_and_record("bob", "5.6.7.8");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, codes::TOO_MANY_ATTEMPTS);
    }

    #[test]
    fn test_rate_limiter_independent_per_username_ip() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_attempts: 2,
            window_secs: 60,
            block_secs: 60,
        });

        limiter.check_and_record("carol", "1.1.1.1").unwrap();
        limiter.check_and_record("carol", "1.1.1.1").unwrap();

        // Different IP → independent counter
        assert!(limiter.check_and_record("carol", "2.2.2.2").is_ok());

        // Different username → independent counter
        assert!(limiter.check_and_record("dave", "1.1.1.1").is_ok());
    }

    #[test]
    fn test_rate_limiter_clear_resets_counter() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_attempts: 2,
            window_secs: 60,
            block_secs: 60,
        });

        limiter.check_and_record("eve", "3.3.3.3").unwrap();
        limiter.check_and_record("eve", "3.3.3.3").unwrap();

        // Clear after successful login
        limiter.clear("eve", "3.3.3.3");

        // Should be allowed again
        assert!(limiter.check_and_record("eve", "3.3.3.3").is_ok());
    }

    #[test]
    fn test_rate_limiter_error_includes_retry_after() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_attempts: 1,
            window_secs: 60,
            block_secs: 60,
        });

        limiter.check_and_record("frank", "4.4.4.4").unwrap();
        let err = limiter.check_and_record("frank", "4.4.4.4").unwrap_err();
        assert!(err.details.is_some());
        let details = err.details.unwrap();
        assert!(details.get("retry_after_secs").is_some());
    }
}
