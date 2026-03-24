use std::fmt;
use std::time::Duration;

use redis::AsyncCommands;
use uuid::Uuid;

// ── Lua script for atomic check-and-delete release ──────────
//
// Prevents releasing a lock held by another process (e.g., ours expired
// and was re-acquired). Only deletes if the stored token matches ours.
const RELEASE_SCRIPT: &str = r#"
if redis.call("get", KEYS[1]) == ARGV[1] then
    return redis.call("del", KEYS[1])
else
    return 0
end
"#;

// ── Error types ─────────────────────────────────────────────

#[derive(Debug)]
pub enum LockError {
    /// Another process holds the lock.
    AlreadyHeld,
    /// Redis is unreachable or returned an unexpected error.
    RedisUnavailable(String),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyHeld => write!(f, "lock already held"),
            Self::RedisUnavailable(msg) => write!(f, "redis unavailable: {msg}"),
        }
    }
}

// ── Configuration ───────────────────────────────────────────

/// Configuration for `acquire_with_retry`.
pub struct RetryConfig {
    pub max_attempts: u32,
    pub retry_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            retry_delay: Duration::from_millis(200),
        }
    }
}

// ── DistributedLock ─────────────────────────────────────────

/// Redis-based distributed lock using SETNX with TTL.
///
/// Acquire: `SET key token NX EX ttl_secs` (atomic, non-blocking).
/// Release: Lua script verifies token before DEL (prevents releasing another's lock).
/// Crash safety: TTL auto-expires the lock if the holder dies without releasing.
pub struct DistributedLock {
    conn: redis::aio::ConnectionManager,
}

impl DistributedLock {
    pub fn new(conn: redis::aio::ConnectionManager) -> Self {
        Self { conn }
    }

    /// Try to acquire the lock once. Returns a `LockGuard` on success.
    pub async fn acquire(&self, key: &str, ttl: Duration) -> Result<LockGuard, LockError> {
        let token = Uuid::now_v7().to_string();
        let ttl_secs = ttl.as_secs().max(1) as u64;

        let result: Option<String> = redis::cmd("SET")
            .arg(key)
            .arg(&token)
            .arg("NX")
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut self.conn.clone())
            .await
            .map_err(|e| LockError::RedisUnavailable(e.to_string()))?;

        // SET NX returns Some("OK") on success, None if key already exists.
        match result {
            Some(_) => Ok(LockGuard {
                conn: self.conn.clone(),
                key: key.to_string(),
                token,
            }),
            None => Err(LockError::AlreadyHeld),
        }
    }

    /// Try to acquire the lock with retries and backoff.
    pub async fn acquire_with_retry(
        &self,
        key: &str,
        ttl: Duration,
        config: RetryConfig,
    ) -> Result<LockGuard, LockError> {
        for attempt in 0..config.max_attempts {
            match self.acquire(key, ttl).await {
                Ok(guard) => return Ok(guard),
                Err(LockError::AlreadyHeld) if attempt + 1 < config.max_attempts => {
                    tokio::time::sleep(config.retry_delay).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(LockError::AlreadyHeld)
    }
}

// ── LockGuard ───────────────────────────────────────────────

/// Handle to a held lock. Call `release()` when done.
///
/// If not explicitly released, the lock auto-expires via its TTL.
/// No Drop impl — async release cannot run in Drop. TTL is the safety net.
pub struct LockGuard {
    conn: redis::aio::ConnectionManager,
    key: String,
    token: String,
}

impl LockGuard {
    /// Release the lock atomically. Returns `true` if we held it, `false` if it
    /// had already expired and been re-acquired by another process.
    pub async fn release(self) -> Result<bool, LockError> {
        let result: i32 = redis::Script::new(RELEASE_SCRIPT)
            .key(&self.key)
            .arg(&self.token)
            .invoke_async(&mut self.conn.clone())
            .await
            .map_err(|e| LockError::RedisUnavailable(e.to_string()))?;

        Ok(result == 1)
    }

    /// The key this guard holds.
    pub fn key(&self) -> &str {
        &self.key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_error_display() {
        assert_eq!(LockError::AlreadyHeld.to_string(), "lock already held");
        assert!(
            LockError::RedisUnavailable("conn refused".into())
                .to_string()
                .contains("conn refused")
        );
    }

    #[test]
    fn retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.retry_delay, Duration::from_millis(200));
    }

    #[test]
    fn ttl_minimum_is_one_second() {
        // Duration::from_millis(500).as_secs() == 0, but we clamp to 1.
        let ttl = Duration::from_millis(500);
        assert_eq!(ttl.as_secs().max(1), 1);
    }
}
