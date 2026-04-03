use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::errors::AppError;

// ── Job name (value object) ────────────────────────────────────────

/// Validated job type identifier following the `{namespace}.{name}` convention.
///
/// Format rules:
/// - Non-empty, max 255 characters
/// - Lowercase alphanumeric, dots, underscores, and hyphens only
/// - Must contain at least one dot (enforces namespacing, e.g. `payment.disburse`)
/// - Must not start or end with a dot
/// - No consecutive dots
///
/// Examples: `payment.disburse`, `catalog.inventory.sync`, `order.cleanup`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobName(String);

impl JobName {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim();

        if trimmed.is_empty() {
            return Err(AppError::BadRequest(
                "Job name must not be empty".to_string(),
            ));
        }

        if trimmed.len() > 255 {
            return Err(AppError::BadRequest(
                "Job name must not exceed 255 characters".to_string(),
            ));
        }

        if !trimmed.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-'
        }) {
            return Err(AppError::BadRequest(
                "Job name must contain only lowercase alphanumeric, dots, underscores, and hyphens"
                    .to_string(),
            ));
        }

        if !trimmed.contains('.') {
            return Err(AppError::BadRequest(
                "Job name must contain at least one dot for namespacing (e.g. 'payment.disburse')"
                    .to_string(),
            ));
        }

        if trimmed.starts_with('.') || trimmed.ends_with('.') {
            return Err(AppError::BadRequest(
                "Job name must not start or end with a dot".to_string(),
            ));
        }

        if trimmed.contains("..") {
            return Err(AppError::BadRequest(
                "Job name must not contain consecutive dots".to_string(),
            ));
        }

        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for JobName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Job status ─────────────────────────────────────────────────────

/// Status of a persistent job in its lifecycle (D1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    DeadLettered,
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::DeadLettered => write!(f, "dead_lettered"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

// ── Job error ──────────────────────────────────────────────────────

/// Error returned by job handlers to distinguish retry-able vs terminal failures (D5).
#[derive(Debug)]
pub enum JobError {
    /// Retry-able failure — will be retried with exponential backoff.
    Transient(String),
    /// Terminal failure — goes directly to dead_lettered.
    Permanent(String),
}

impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient(msg) => write!(f, "transient: {msg}"),
            Self::Permanent(msg) => write!(f, "permanent: {msg}"),
        }
    }
}

// ── Dedup strategy ─────────────────────────────────────────────────

/// Deduplication strategy for one-shot jobs (D8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DedupStrategy {
    /// Reject duplicate enqueue silently (default).
    #[default]
    Skip,
    /// No dedup — multiple jobs with same type coexist.
    Enqueue,
    /// Cancel existing pending job and insert new one.
    Replace,
}

// ── Job (DB row) ───────────────────────────────────────────────────

/// A row from the `persistent_jobs` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub job_type: String,
    pub payload: Value,
    pub status: JobStatus,
    pub schedule: Option<String>,
    pub dedup_key: Option<String>,
    pub max_retries: i32,
    pub timeout_seconds: i32,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub locked_by: Option<String>,
    pub locked_at: Option<DateTime<Utc>>,
    pub next_run_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── Job insert DTO ─────────────────────────────────────────────────

/// Data required to enqueue a new one-shot job (D12).
pub struct JobInsert {
    pub job_type: String,
    pub payload: Value,
    pub config: Option<JobConfig>,
}

// ── Job config (per-job overrides) ─────────────────────────────────

/// Per-job configuration overrides (D13).
#[derive(Debug, Clone, Default)]
pub struct JobConfig {
    pub max_retries: Option<u32>,
    pub timeout_seconds: Option<u32>,
    pub dedup_strategy: DedupStrategy,
    pub dedup_key: Option<String>,
}

// ── Recurring job types ────────────────────────────────────────────

/// Schedule for recurring jobs (D7, D11).
#[derive(Debug, Clone)]
pub enum JobSchedule {
    /// 6-field cron expression (sec min hour dom month dow).
    Cron(cron::Schedule),
    /// Fixed interval between runs.
    Interval(std::time::Duration),
}

/// What to do when a recurring job exhausts all retries (D7).
#[derive(Debug, Clone, Default)]
pub enum RecurringFailurePolicy {
    /// Go to `failed` — slot is dead until operator retries (default).
    #[default]
    Die,
    /// Reset to `pending` with next cron/interval tick — continue scheduling.
    ResetToNext,
}

/// Definition for a recurring job registered at startup (D7).
pub struct RecurringJobDefinition {
    pub job_name: JobName,
    pub schedule: JobSchedule,
    pub payload: Value,
    pub dedup_key: String,
    pub config: Option<JobConfig>,
    pub failure_policy: RecurringFailurePolicy,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── JobName tests ──────────────────────────────────────────

    #[test]
    fn job_name_valid() {
        assert!(JobName::new("payment.disburse").is_ok());
        assert!(JobName::new("catalog.inventory.sync").is_ok());
        assert!(JobName::new("order.cleanup").is_ok());
        assert!(JobName::new("test.job-1").is_ok());
        assert!(JobName::new("test.job_2").is_ok());
        assert!(JobName::new("a.b").is_ok());
    }

    #[test]
    fn job_name_rejects_empty() {
        let err = JobName::new("").unwrap_err().to_string();
        assert!(err.contains("empty"), "got: {err}");
    }

    #[test]
    fn job_name_rejects_whitespace_only() {
        assert!(JobName::new("   ").is_err());
    }

    #[test]
    fn job_name_rejects_no_dot() {
        let err = JobName::new("paymentdisburse").unwrap_err().to_string();
        assert!(err.contains("dot"), "got: {err}");
    }

    #[test]
    fn job_name_rejects_uppercase() {
        assert!(JobName::new("Payment.disburse").is_err());
    }

    #[test]
    fn job_name_rejects_spaces() {
        assert!(JobName::new("payment .disburse").is_err());
    }

    #[test]
    fn job_name_rejects_leading_dot() {
        assert!(JobName::new(".payment.disburse").is_err());
    }

    #[test]
    fn job_name_rejects_trailing_dot() {
        assert!(JobName::new("payment.disburse.").is_err());
    }

    #[test]
    fn job_name_rejects_consecutive_dots() {
        assert!(JobName::new("payment..disburse").is_err());
    }

    #[test]
    fn job_name_rejects_too_long() {
        let long = format!("a.{}", "b".repeat(254));
        assert!(JobName::new(&long).is_err());
    }

    #[test]
    fn job_name_trims_whitespace() {
        let name = JobName::new("  payment.disburse  ").unwrap();
        assert_eq!(name.as_str(), "payment.disburse");
    }

    #[test]
    fn job_name_display() {
        let name = JobName::new("payment.disburse").unwrap();
        assert_eq!(name.to_string(), "payment.disburse");
    }

    // ── Other type tests ───────────────────────────────────────

    #[test]
    fn job_status_display() {
        assert_eq!(JobStatus::Pending.to_string(), "pending");
        assert_eq!(JobStatus::Running.to_string(), "running");
        assert_eq!(JobStatus::Completed.to_string(), "completed");
        assert_eq!(JobStatus::Failed.to_string(), "failed");
        assert_eq!(JobStatus::DeadLettered.to_string(), "dead_lettered");
        assert_eq!(JobStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn job_error_display() {
        let t = JobError::Transient("db down".into());
        assert!(t.to_string().contains("transient"));
        let p = JobError::Permanent("bad data".into());
        assert!(p.to_string().contains("permanent"));
    }

    #[test]
    fn dedup_strategy_default_is_skip() {
        assert_eq!(DedupStrategy::default(), DedupStrategy::Skip);
    }

    #[test]
    fn job_config_default() {
        let c = JobConfig::default();
        assert!(c.max_retries.is_none());
        assert!(c.timeout_seconds.is_none());
        assert_eq!(c.dedup_strategy, DedupStrategy::Skip);
        assert!(c.dedup_key.is_none());
    }
}
