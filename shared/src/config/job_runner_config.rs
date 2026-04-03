use std::time::Duration;
use uuid::Uuid;

use super::{parse_env_or, read_env_or};

/// Configuration for the job runner background task (D13).
///
/// Follows the same `from_env()` + `Default` pattern as `RelayConfig`.
pub struct JobRunnerConfig {
    /// Unique identifier for this runner instance (for lock ownership).
    pub instance_id: String,
    /// Maximum concurrent jobs this runner will execute.
    pub max_concurrent_jobs: usize,
    /// Fallback polling interval when LISTEN/NOTIFY is missed.
    pub poll_interval: Duration,
    /// How often the stale lock recovery loop runs.
    pub stale_lock_check_interval: Duration,
    /// How long before a running job's lock is considered stale.
    pub stale_lock_timeout: Duration,
    /// How often the cleanup maintenance loop runs.
    pub cleanup_interval: Duration,
    /// Maximum age of completed jobs before cleanup deletes them.
    pub cleanup_max_age: Duration,
    /// Default max retries for jobs without per-job override.
    pub default_max_retries: u32,
    /// Default timeout in seconds for jobs without per-job override.
    pub default_timeout_seconds: u32,
}

impl JobRunnerConfig {
    /// Build from environment variables, falling back to sensible defaults.
    ///
    /// | Variable | Type | Default |
    /// |----------|------|---------|
    /// | `JOB_RUNNER_INSTANCE_ID` | String | UUID v7 |
    /// | `JOB_RUNNER_MAX_CONCURRENT_JOBS` | usize | 5 |
    /// | `JOB_RUNNER_POLL_INTERVAL_MS` | u64 | 1000 |
    /// | `JOB_RUNNER_STALE_LOCK_CHECK_INTERVAL_SECS` | u64 | 30 |
    /// | `JOB_RUNNER_STALE_LOCK_TIMEOUT_SECS` | u64 | 300 |
    /// | `JOB_RUNNER_CLEANUP_INTERVAL_SECS` | u64 | 3600 |
    /// | `JOB_RUNNER_CLEANUP_MAX_AGE_SECS` | u64 | 604800 (7 days) |
    /// | `JOB_RUNNER_DEFAULT_MAX_RETRIES` | u32 | 5 |
    /// | `JOB_RUNNER_DEFAULT_TIMEOUT_SECS` | u32 | 300 |
    pub fn from_env() -> Self {
        let max_concurrent: usize = parse_env_or("JOB_RUNNER_MAX_CONCURRENT_JOBS", 5);
        let poll_interval_ms: u64 = parse_env_or("JOB_RUNNER_POLL_INTERVAL_MS", 1000);
        let stale_lock_check_secs: u64 =
            parse_env_or("JOB_RUNNER_STALE_LOCK_CHECK_INTERVAL_SECS", 30);
        let stale_lock_timeout_secs: u64 = parse_env_or("JOB_RUNNER_STALE_LOCK_TIMEOUT_SECS", 300);
        let cleanup_interval_secs: u64 = parse_env_or("JOB_RUNNER_CLEANUP_INTERVAL_SECS", 3600);
        let cleanup_max_age_secs: u64 =
            parse_env_or("JOB_RUNNER_CLEANUP_MAX_AGE_SECS", 7 * 24 * 3600);

        assert!(
            max_concurrent > 0,
            "JOB_RUNNER_MAX_CONCURRENT_JOBS must be positive"
        );
        assert!(
            poll_interval_ms > 0,
            "JOB_RUNNER_POLL_INTERVAL_MS must be positive"
        );
        assert!(
            stale_lock_timeout_secs > stale_lock_check_secs,
            "stale_lock_timeout must exceed stale_lock_check_interval"
        );

        Self {
            instance_id: read_env_or("JOB_RUNNER_INSTANCE_ID", Uuid::now_v7().to_string()),
            max_concurrent_jobs: max_concurrent,
            poll_interval: Duration::from_millis(poll_interval_ms),
            stale_lock_check_interval: Duration::from_secs(stale_lock_check_secs),
            stale_lock_timeout: Duration::from_secs(stale_lock_timeout_secs),
            cleanup_interval: Duration::from_secs(cleanup_interval_secs),
            cleanup_max_age: Duration::from_secs(cleanup_max_age_secs),
            default_max_retries: parse_env_or("JOB_RUNNER_DEFAULT_MAX_RETRIES", 5),
            default_timeout_seconds: parse_env_or("JOB_RUNNER_DEFAULT_TIMEOUT_SECS", 300),
        }
    }
}

impl Default for JobRunnerConfig {
    fn default() -> Self {
        Self {
            instance_id: Uuid::now_v7().to_string(),
            max_concurrent_jobs: 5,
            poll_interval: Duration::from_millis(1000),
            stale_lock_check_interval: Duration::from_secs(30),
            stale_lock_timeout: Duration::from_secs(300),
            cleanup_interval: Duration::from_secs(3600),
            cleanup_max_age: Duration::from_secs(7 * 24 * 3600),
            default_max_retries: 5,
            default_timeout_seconds: 300,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_runner_config_defaults() {
        let config = JobRunnerConfig::default();

        assert_eq!(config.max_concurrent_jobs, 5);
        assert_eq!(config.poll_interval, Duration::from_millis(1000));
        assert_eq!(config.stale_lock_check_interval, Duration::from_secs(30));
        assert_eq!(config.stale_lock_timeout, Duration::from_secs(300));
        assert_eq!(config.cleanup_interval, Duration::from_secs(3600));
        assert_eq!(config.cleanup_max_age, Duration::from_secs(7 * 24 * 3600));
        assert_eq!(config.default_max_retries, 5);
        assert_eq!(config.default_timeout_seconds, 300);
        assert!(!config.instance_id.is_empty());
    }

    #[test]
    fn job_runner_config_from_env_reads_overrides() {
        // SAFETY: test-only, single-threaded via --test-threads=1
        unsafe {
            std::env::set_var("JOB_RUNNER_MAX_CONCURRENT_JOBS", "10");
            std::env::set_var("JOB_RUNNER_POLL_INTERVAL_MS", "500");
            std::env::set_var("JOB_RUNNER_INSTANCE_ID", "runner-42");
            std::env::set_var("JOB_RUNNER_DEFAULT_MAX_RETRIES", "3");
            std::env::set_var("JOB_RUNNER_DEFAULT_TIMEOUT_SECS", "60");
        }

        let config = JobRunnerConfig::from_env();

        assert_eq!(config.max_concurrent_jobs, 10);
        assert_eq!(config.poll_interval, Duration::from_millis(500));
        assert_eq!(config.instance_id, "runner-42");
        assert_eq!(config.default_max_retries, 3);
        assert_eq!(config.default_timeout_seconds, 60);

        // Non-overridden fields use defaults
        assert_eq!(config.stale_lock_timeout, Duration::from_secs(300));
        assert_eq!(config.cleanup_interval, Duration::from_secs(3600));

        unsafe {
            std::env::remove_var("JOB_RUNNER_MAX_CONCURRENT_JOBS");
            std::env::remove_var("JOB_RUNNER_POLL_INTERVAL_MS");
            std::env::remove_var("JOB_RUNNER_INSTANCE_ID");
            std::env::remove_var("JOB_RUNNER_DEFAULT_MAX_RETRIES");
            std::env::remove_var("JOB_RUNNER_DEFAULT_TIMEOUT_SECS");
        }
    }
}
