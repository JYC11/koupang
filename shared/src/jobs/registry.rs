use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::db::PgPool;
use crate::jobs::types::{JobError, JobName, RecurringJobDefinition};

// ── Job handler trait ──────────────────────────────────────────────

/// Trait for job handlers (D4).
///
/// Handlers receive the job payload and a pool reference. They manage their own
/// transactions via `with_transaction()`. Idempotency is a documented requirement
/// — at-least-once delivery means a job may be re-executed if the runner crashes
/// between handler success and completion marking.
#[async_trait::async_trait]
pub trait JobHandler: Send + Sync {
    /// The job type string this handler processes (must match `job_type` column).
    fn job_type(&self) -> &str;

    /// Execute the job. Returns `Ok(())` on success, or `JobError` to signal
    /// whether the failure is transient (retry) or permanent (dead-letter).
    async fn execute(&self, payload: &Value, pool: &PgPool) -> Result<(), JobError>;
}

// ── Job registry ───────────────────────────────────────────────────

/// Registry mapping job type strings to handler implementations.
pub struct JobRegistry {
    handlers: HashMap<String, Arc<dyn JobHandler>>,
    recurring: Vec<RecurringJobDefinition>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            recurring: Vec::new(),
        }
    }

    /// Register a handler. Panics if the job_type format is invalid or if a
    /// handler for the same `job_type` is already registered.
    pub fn register(&mut self, handler: Arc<dyn JobHandler>) {
        let raw = handler.job_type();
        let job_name =
            JobName::new(raw).unwrap_or_else(|e| panic!("invalid job_type format '{raw}': {e}"));
        let key = job_name.into_inner();
        if self.handlers.contains_key(&key) {
            panic!("duplicate job handler registration for type: {key}");
        }
        self.handlers.insert(key, handler);
    }

    /// Register a recurring job definition. The handler for `def.job_name` must
    /// already be registered via `register()`. Panics otherwise.
    pub fn register_recurring(&mut self, def: RecurringJobDefinition) {
        let key = def.job_name.as_str();
        assert!(
            self.handlers.contains_key(key),
            "handler for '{}' must be registered before register_recurring()",
            key
        );
        self.recurring.push(def);
    }

    /// Look up a handler by job type.
    pub fn get(&self, job_type: &str) -> Option<Arc<dyn JobHandler>> {
        self.handlers.get(job_type).cloned()
    }

    /// Access recurring job definitions (used by runner at startup).
    pub fn recurring_definitions(&self) -> &[RecurringJobDefinition] {
        &self.recurring
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHandler;

    #[async_trait::async_trait]
    impl JobHandler for TestHandler {
        fn job_type(&self) -> &str {
            "test.job"
        }
        async fn execute(&self, _payload: &Value, _pool: &PgPool) -> Result<(), JobError> {
            Ok(())
        }
    }

    #[test]
    fn registry_register_and_get() {
        let mut registry = JobRegistry::new();
        registry.register(Arc::new(TestHandler));

        assert!(registry.get("test.job").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    #[should_panic(expected = "duplicate job handler registration")]
    fn registry_rejects_duplicate() {
        let mut registry = JobRegistry::new();
        registry.register(Arc::new(TestHandler));
        registry.register(Arc::new(TestHandler));
    }

    #[test]
    fn job_handler_is_send_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn JobHandler>();
    }
}
