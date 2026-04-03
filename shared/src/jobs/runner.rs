use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sqlx::postgres::PgListener;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::config::job_runner_config::JobRunnerConfig;
use crate::db::PgPool;
use crate::jobs::registry::JobRegistry;
use crate::jobs::repository::{claim_batch, mark_completed};

/// Background job runner (D6).
///
/// Runs a claim-and-execute loop that processes pending jobs from the
/// `persistent_jobs` table. Uses PgListener NOTIFY for immediate wakeup
/// on new job insertion, with poll_interval as fallback.
///
/// Phase 1 runs only the claim_and_execute_loop. Phases 2-3 add error
/// handling, stale lock recovery, and cleanup loops.
pub struct JobRunner {
    pool: PgPool,
    registry: JobRegistry,
    config: JobRunnerConfig,
    in_flight: AtomicUsize,
    drain_notify: Notify,
}

/// RAII guard that decrements the in-flight counter and signals the drain
/// notification on Drop — including on panic (D6, eng review).
///
/// Uses `Arc<JobRunner>` so the guard is `'static` and can be moved into
/// `tokio::spawn` tasks.
struct InFlightGuard {
    runner: Arc<JobRunner>,
}

impl InFlightGuard {
    fn new(runner: &Arc<JobRunner>) -> Self {
        runner.in_flight.fetch_add(1, Ordering::Relaxed);
        Self {
            runner: Arc::clone(runner),
        }
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.runner.in_flight.fetch_sub(1, Ordering::Relaxed);
        self.runner.drain_notify.notify_waiters();
    }
}

impl JobRunner {
    pub fn new(pool: PgPool, registry: JobRegistry, config: JobRunnerConfig) -> Self {
        Self {
            pool,
            registry,
            config,
            in_flight: AtomicUsize::new(0),
            drain_notify: Notify::new(),
        }
    }

    /// Start the runner. Runs until the cancellation token is triggered.
    ///
    /// Phase 1: only the claim_and_execute_loop runs.
    /// Phase 3 will add stale_lock_recovery_loop and cleanup_loop with tokio::join!.
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken) {
        let runner = Arc::clone(&self);
        let s = shutdown.clone();

        let claim_handle = tokio::spawn(async move {
            Self::claim_and_execute_loop(runner, s).await;
        });

        let _ = claim_handle.await;

        // Drain phase: wait for all in-flight jobs to complete
        self.drain(shutdown).await;

        tracing::info!("Job runner shut down gracefully");
    }

    /// Wait for in-flight jobs to complete (with safety timeout).
    async fn drain(&self, _shutdown: CancellationToken) {
        let timeout = self.config.stale_lock_timeout;
        let deadline = tokio::time::Instant::now() + timeout;

        while self.in_flight.load(Ordering::Relaxed) > 0 {
            tokio::select! {
                biased;
                _ = self.drain_notify.notified() => {}
                _ = tokio::time::sleep_until(deadline) => {
                    let remaining = self.in_flight.load(Ordering::Relaxed);
                    tracing::warn!(
                        remaining,
                        "Drain safety timeout reached, proceeding with shutdown"
                    );
                    break;
                }
            }
        }
    }

    // ── Claim and execute loop ─────────────────────────────────────

    async fn claim_and_execute_loop(runner: Arc<Self>, shutdown: CancellationToken) {
        let mut listener = Self::connect_listener(&runner.pool).await;

        loop {
            tokio::select! {
                biased;

                _ = shutdown.cancelled() => {
                    tracing::info!("Job claim loop: shutdown signal received");
                    return;
                }

                notification = async {
                    match listener.as_mut() {
                        Some(l) => l.recv().await.map(Some),
                        None => std::future::pending().await,
                    }
                } => {
                    match notification {
                        Ok(Some(_)) => {
                            tracing::debug!("Job runner woken by PG notification");
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "PgListener error, attempting reconnect");
                            listener = Self::connect_listener(&runner.pool).await;
                        }
                        _ => {}
                    }
                }

                _ = tokio::time::sleep(runner.config.poll_interval) => {
                    tracing::trace!("Job runner woken by poll interval");
                    if listener.is_none() {
                        listener = Self::connect_listener(&runner.pool).await;
                    }
                }
            }

            runner.process_available(&shutdown).await;
        }
    }

    /// Claim and execute available jobs up to capacity.
    async fn process_available(self: &Arc<Self>, shutdown: &CancellationToken) {
        if shutdown.is_cancelled() {
            return;
        }

        let current = self.in_flight.load(Ordering::Relaxed);
        let capacity = self.config.max_concurrent_jobs.saturating_sub(current) as i32;
        if capacity <= 0 {
            return;
        }

        let jobs = match claim_batch(&self.pool, capacity, &self.config.instance_id).await {
            Ok(jobs) => jobs,
            Err(e) => {
                tracing::error!(error = %e, "Failed to claim job batch");
                return;
            }
        };

        if jobs.is_empty() {
            return;
        }

        tracing::debug!(count = jobs.len(), "Claimed jobs for execution");

        for job in jobs {
            let pool = self.pool.clone();
            let handler = match self.registry.get(&job.job_type) {
                Some(h) => h,
                None => {
                    tracing::error!(job_type = %job.job_type, job_id = %job.id, "No handler registered");
                    // Leave as running — stale lock recovery will free it (Phase 3)
                    continue;
                }
            };

            let guard = InFlightGuard::new(self);

            tokio::spawn(async move {
                let _guard = guard;
                let job_id = job.id;
                let job_type = job.job_type.clone();

                match handler.execute(&job.payload, &pool).await {
                    Ok(()) => {
                        if let Err(e) = mark_completed(&pool, job_id).await {
                            tracing::error!(
                                job_id = %job_id,
                                error = %e,
                                "Failed to mark job completed"
                            );
                        } else {
                            tracing::debug!(job_id = %job_id, job_type = %job_type, "Job completed");
                        }
                    }
                    Err(e) => {
                        // Phase 1: log only. Phase 2 adds retry/dead-letter handling.
                        tracing::error!(
                            job_id = %job_id,
                            job_type = %job_type,
                            error = %e,
                            "Job handler failed (retry handling deferred to Phase 2)"
                        );
                    }
                }
            });
        }
    }

    // ── PgListener setup ───────────────────────────────────────────

    async fn connect_listener(pool: &PgPool) -> Option<PgListener> {
        match PgListener::connect_with(pool).await {
            Ok(mut listener) => match listener.listen("persistent_jobs").await {
                Ok(()) => {
                    tracing::info!("PgListener connected, listening on 'persistent_jobs' channel");
                    Some(listener)
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "Failed to listen on persistent_jobs channel, using poll-only mode"
                    );
                    None
                }
            },
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Failed to connect PgListener, using poll-only mode"
                );
                None
            }
        }
    }
}

// InFlightGuard is tested via integration tests (handler_panic_safety in job_tests.rs)
// and implicitly by runner_end_to_end. Unit tests for the guard require a PgPool,
// so they live in the integration test file.
