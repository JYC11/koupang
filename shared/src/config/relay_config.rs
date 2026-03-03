use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::outbox::types::FailureEscalation;

use super::{env_or, env_parse};

/// Configuration for the outbox relay background task.
pub struct RelayConfig {
    /// Unique identifier for this relay instance (for lock ownership).
    pub instance_id: String,
    /// Maximum events to claim per batch.
    pub batch_size: i64,
    /// Fallback polling interval when LISTEN/NOTIFY is missed.
    pub poll_interval: Duration,
    /// How often the stale lock recovery loop runs.
    pub stale_lock_check_interval: Duration,
    /// How long before a lock is considered stale (dead relay detection).
    pub stale_lock_timeout: Duration,
    /// How often the cleanup maintenance loop runs.
    pub cleanup_interval: Duration,
    /// Maximum age of published events before cleanup deletes them.
    pub cleanup_max_age: Duration,
    /// When true, DELETE rows immediately after successful publish
    /// instead of marking as 'published'. Reduces table bloat for
    /// high-throughput services.
    pub delete_on_publish: bool,
    /// Optional handler invoked when an event exhausts all retries.
    /// Defaults to `LogFailureEscalation` if not provided.
    pub failure_escalation: Option<Arc<dyn FailureEscalation>>,
}

impl RelayConfig {
    /// Build from environment variables, falling back to sensible defaults.
    ///
    /// | Variable | Type | Default |
    /// |----------|------|---------|
    /// | `OUTBOX_RELAY_INSTANCE_ID` | String | UUID v7 |
    /// | `OUTBOX_RELAY_BATCH_SIZE` | i64 | 50 |
    /// | `OUTBOX_RELAY_POLL_INTERVAL_MS` | u64 | 500 |
    /// | `OUTBOX_RELAY_STALE_LOCK_CHECK_INTERVAL_SECS` | u64 | 30 |
    /// | `OUTBOX_RELAY_STALE_LOCK_TIMEOUT_SECS` | u64 | 60 |
    /// | `OUTBOX_RELAY_CLEANUP_INTERVAL_SECS` | u64 | 3600 |
    /// | `OUTBOX_RELAY_CLEANUP_MAX_AGE_SECS` | u64 | 604800 (7 days) |
    /// | `OUTBOX_RELAY_DELETE_ON_PUBLISH` | bool | false |
    pub fn from_env() -> Self {
        Self {
            instance_id: env_or("OUTBOX_RELAY_INSTANCE_ID", Uuid::now_v7().to_string()),
            batch_size: env_parse("OUTBOX_RELAY_BATCH_SIZE", 50),
            poll_interval: Duration::from_millis(env_parse("OUTBOX_RELAY_POLL_INTERVAL_MS", 500)),
            stale_lock_check_interval: Duration::from_secs(env_parse(
                "OUTBOX_RELAY_STALE_LOCK_CHECK_INTERVAL_SECS",
                30,
            )),
            stale_lock_timeout: Duration::from_secs(env_parse(
                "OUTBOX_RELAY_STALE_LOCK_TIMEOUT_SECS",
                60,
            )),
            cleanup_interval: Duration::from_secs(env_parse(
                "OUTBOX_RELAY_CLEANUP_INTERVAL_SECS",
                3600,
            )),
            cleanup_max_age: Duration::from_secs(env_parse(
                "OUTBOX_RELAY_CLEANUP_MAX_AGE_SECS",
                7 * 24 * 3600,
            )),
            delete_on_publish: env_parse("OUTBOX_RELAY_DELETE_ON_PUBLISH", false),
            failure_escalation: None,
        }
    }
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            instance_id: Uuid::now_v7().to_string(),
            batch_size: 50,
            poll_interval: Duration::from_millis(500),
            stale_lock_check_interval: Duration::from_secs(30),
            stale_lock_timeout: Duration::from_secs(60),
            cleanup_interval: Duration::from_secs(3600),
            cleanup_max_age: Duration::from_secs(7 * 24 * 3600), // 7 days
            delete_on_publish: false,
            failure_escalation: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_config_defaults() {
        let config = RelayConfig::default();

        assert_eq!(config.batch_size, 50);
        assert_eq!(config.poll_interval, Duration::from_millis(500));
        assert_eq!(config.stale_lock_check_interval, Duration::from_secs(30));
        assert_eq!(config.stale_lock_timeout, Duration::from_secs(60));
        assert_eq!(config.cleanup_interval, Duration::from_secs(3600));
        assert_eq!(config.cleanup_max_age, Duration::from_secs(7 * 24 * 3600));
        assert!(!config.delete_on_publish);
        assert!(config.failure_escalation.is_none());
        assert!(!config.instance_id.is_empty());
    }

    #[test]
    fn relay_config_from_env_reads_overrides() {
        // SAFETY: test-only, single-threaded via --test-threads=1
        unsafe {
            std::env::set_var("OUTBOX_RELAY_BATCH_SIZE", "100");
            std::env::set_var("OUTBOX_RELAY_POLL_INTERVAL_MS", "250");
            std::env::set_var("OUTBOX_RELAY_DELETE_ON_PUBLISH", "true");
            std::env::set_var("OUTBOX_RELAY_INSTANCE_ID", "relay-42");
        }

        let config = RelayConfig::from_env();

        assert_eq!(config.batch_size, 100);
        assert_eq!(config.poll_interval, Duration::from_millis(250));
        assert!(config.delete_on_publish);
        assert_eq!(config.instance_id, "relay-42");

        // Non-overridden fields use defaults
        assert_eq!(config.stale_lock_timeout, Duration::from_secs(60));
        assert_eq!(config.cleanup_interval, Duration::from_secs(3600));

        unsafe {
            std::env::remove_var("OUTBOX_RELAY_BATCH_SIZE");
            std::env::remove_var("OUTBOX_RELAY_POLL_INTERVAL_MS");
            std::env::remove_var("OUTBOX_RELAY_DELETE_ON_PUBLISH");
            std::env::remove_var("OUTBOX_RELAY_INSTANCE_ID");
        }
    }

    #[test]
    fn relay_config_from_env_ignores_invalid_values() {
        // SAFETY: test-only, single-threaded via --test-threads=1
        unsafe {
            std::env::set_var("OUTBOX_RELAY_BATCH_SIZE", "not_a_number");
        }

        let config = RelayConfig::from_env();
        assert_eq!(config.batch_size, 50); // falls back to default

        unsafe {
            std::env::remove_var("OUTBOX_RELAY_BATCH_SIZE");
        }
    }
}
