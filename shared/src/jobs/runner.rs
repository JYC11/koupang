use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sqlx::postgres::PgListener;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::config::job_runner_config::JobRunnerConfig;
use crate::db::PgPool;
use crate::jobs::registry::JobRegistry;
use crate::jobs::repository::{
    claim_batch, cleanup_completed, mark_completed, mark_dead_lettered, mark_retry_or_failed,
    release_stale_locks, reset_recurring, seed_recurring_job,
};
use crate::jobs::types::{JobError, JobSchedule, RecurringFailurePolicy};

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

    /// Start the runner. Runs until the cancellation token is triggered (D6).
    ///
    /// Spawns three concurrent loops:
    /// 1. **claim_and_execute_loop** — main work (claim → execute → mark)
    /// 2. **stale_lock_recovery_loop** — free crashed/timed-out jobs
    /// 3. **cleanup_loop** — delete old completed jobs in batches
    ///
    /// After all loops exit, waits for in-flight jobs to finish (drain phase).
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken) {
        // Seed recurring job slots at startup (D7)
        self.seed_recurring_jobs().await;

        let claim_handle = {
            let r = Arc::clone(&self);
            let s = shutdown.clone();
            tokio::spawn(async move { Self::claim_and_execute_loop(r, s).await })
        };

        let stale_handle = {
            let r = Arc::clone(&self);
            let s = shutdown.clone();
            tokio::spawn(async move { Self::stale_lock_recovery_loop(r, s).await })
        };

        let cleanup_handle = {
            let r = Arc::clone(&self);
            let s = shutdown.clone();
            tokio::spawn(async move { Self::cleanup_loop(r, s).await })
        };

        let _ = tokio::join!(claim_handle, stale_handle, cleanup_handle);

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
            let timeout_secs = job.timeout_seconds as u64;
            let max_retries = job.max_retries;
            let is_recurring = job.schedule.is_some();
            // Look up recurring definition for failure policy (if applicable)
            let recurring_def = if is_recurring {
                self.registry
                    .recurring_definitions()
                    .iter()
                    .find(|d| d.job_name.as_str() == job.job_type)
            } else {
                None
            };
            let failure_policy = recurring_def
                .map(|d| d.failure_policy.clone())
                .unwrap_or_default();
            let schedule = recurring_def.map(|d| d.schedule.clone());

            tokio::spawn(async move {
                let _guard = guard;
                let job_id = job.id;
                let job_type = job.job_type.clone();

                let timeout_duration = std::time::Duration::from_secs(timeout_secs);
                let result =
                    tokio::time::timeout(timeout_duration, handler.execute(&job.payload, &pool))
                        .await;

                match result {
                    // Handler succeeded
                    Ok(Ok(())) => {
                        if let Err(e) = mark_completed(&pool, job_id).await {
                            tracing::error!(
                                job_id = %job_id,
                                error = %e,
                                "Failed to mark job completed"
                            );
                        } else {
                            tracing::debug!(job_id = %job_id, job_type = %job_type, "Job completed");
                        }

                        // Recurring: reset slot to pending with next run time
                        if let Some(ref sched) = schedule {
                            let missed = count_missed_ticks(sched, chrono::Utc::now());
                            if missed > 0 {
                                tracing::warn!(
                                    job_id = %job_id,
                                    job_type = %job_type,
                                    missed_ticks = missed,
                                    "Recurring job overran schedule, skipping missed ticks"
                                );
                            }
                            let next = compute_next_run_at(sched);
                            if let Err(e) = reset_recurring(&pool, job_id, next).await {
                                tracing::error!(
                                    job_id = %job_id,
                                    error = %e,
                                    "Failed to reset recurring job"
                                );
                            }
                        }
                    }
                    // Transient error — retry with backoff or fail if exhausted
                    Ok(Err(JobError::Transient(msg))) => {
                        tracing::warn!(
                            job_id = %job_id,
                            job_type = %job_type,
                            attempts = job.attempts,
                            max_retries,
                            error = %msg,
                            "Job failed with transient error"
                        );
                        if let Err(e) = mark_retry_or_failed(&pool, job_id, &msg, max_retries).await
                        {
                            tracing::error!(
                                job_id = %job_id,
                                error = %e,
                                "Failed to mark job retry/failed"
                            );
                        }

                        // Recurring + retries exhausted: check failure policy
                        if is_recurring && job.attempts >= max_retries {
                            match failure_policy {
                                RecurringFailurePolicy::Die => {
                                    tracing::error!(
                                        job_id = %job_id,
                                        job_type = %job_type,
                                        "Recurring job slot dead (Die policy)"
                                    );
                                }
                                RecurringFailurePolicy::ResetToNext => {
                                    if let Some(ref sched) = schedule {
                                        let next = compute_next_run_at(sched);
                                        tracing::warn!(
                                            job_id = %job_id,
                                            job_type = %job_type,
                                            ?next,
                                            "Recurring job retries exhausted, resetting to next tick (ResetToNext policy)"
                                        );
                                        if let Err(e) = reset_recurring(&pool, job_id, next).await {
                                            tracing::error!(
                                                job_id = %job_id,
                                                error = %e,
                                                "Failed to reset recurring job after exhaustion"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Permanent error — dead-letter immediately
                    Ok(Err(JobError::Permanent(msg))) => {
                        tracing::error!(
                            job_id = %job_id,
                            job_type = %job_type,
                            error = %msg,
                            "Job failed with permanent error, dead-lettering"
                        );
                        if let Err(e) = mark_dead_lettered(&pool, job_id, &msg).await {
                            tracing::error!(
                                job_id = %job_id,
                                error = %e,
                                "Failed to mark job dead-lettered"
                            );
                        }
                    }
                    // Timeout — leave as running, stale lock recovery handles it
                    Err(_elapsed) => {
                        tracing::warn!(
                            job_id = %job_id,
                            job_type = %job_type,
                            timeout_secs,
                            "Job timed out, leaving as running for stale lock recovery"
                        );
                    }
                }
            });
        }
    }

    // ── Stale lock recovery loop ─────────────────────────────────

    async fn stale_lock_recovery_loop(runner: Arc<Self>, shutdown: CancellationToken) {
        let timeout_secs = runner.config.stale_lock_timeout.as_secs() as i64;

        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => {
                    tracing::info!("Stale lock loop: shutdown signal received");
                    return;
                }
                _ = tokio::time::sleep(runner.config.stale_lock_check_interval) => {}
            }

            match release_stale_locks(&runner.pool, timeout_secs).await {
                Ok(0) => {}
                Ok(n) => tracing::info!(count = n, "Released stale job locks"),
                Err(e) => tracing::error!(error = %e, "Failed to release stale locks"),
            }
        }
    }

    // ── Cleanup loop ──────────────────────────────────────────────

    async fn cleanup_loop(runner: Arc<Self>, shutdown: CancellationToken) {
        let max_age_secs = runner.config.cleanup_max_age.as_secs() as i64;

        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => {
                    tracing::info!("Cleanup loop: shutdown signal received");
                    return;
                }
                _ = tokio::time::sleep(runner.config.cleanup_interval) => {}
            }

            // Drain in batches of 1000 to avoid long transactions.
            let mut total = 0u64;
            loop {
                if shutdown.is_cancelled() {
                    break;
                }
                match cleanup_completed(&runner.pool, max_age_secs).await {
                    Ok(0) => break,
                    Ok(n) => total += n,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to cleanup completed jobs");
                        break;
                    }
                }
            }
            if total > 0 {
                tracing::info!(count = total, "Cleaned up old completed jobs");
            }
        }
    }

    // ── Recurring job support ────────────────────────────────────

    /// Seed recurring job slots at startup (D7).
    async fn seed_recurring_jobs(&self) {
        for def in self.registry.recurring_definitions() {
            match seed_recurring_job(&self.pool, def).await {
                Ok(Some(id)) => {
                    tracing::info!(
                        job_type = %def.job_name,
                        job_id = %id,
                        "Seeded recurring job slot"
                    );
                }
                Ok(None) => {
                    tracing::debug!(
                        job_type = %def.job_name,
                        "Recurring job slot already exists"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        job_type = %def.job_name,
                        error = %e,
                        "Failed to seed recurring job"
                    );
                }
            }
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

// ── Recurring schedule helpers ─────────────────────────────────────

/// Compute the next run time from the current moment.
///
/// - Cron: iterate schedule from `now`, take the first future tick.
/// - Interval: `now + duration`.
pub fn compute_next_run_at(schedule: &JobSchedule) -> chrono::DateTime<chrono::Utc> {
    let now = chrono::Utc::now();
    match schedule {
        JobSchedule::Cron(s) => s
            .upcoming(chrono::Utc)
            .next()
            .expect("cron schedule should always have a next tick"),
        JobSchedule::Interval(d) => now + chrono::Duration::from_std(*d).unwrap(),
    }
}

/// Count how many schedule ticks were missed between `last_run` and now.
///
/// Returns 0 if no ticks were missed (i.e., we're on schedule).
pub fn count_missed_ticks(
    schedule: &JobSchedule,
    last_run: chrono::DateTime<chrono::Utc>,
) -> usize {
    let now = chrono::Utc::now();
    match schedule {
        JobSchedule::Cron(s) => {
            // Count ticks between last_run and now
            s.after(&last_run)
                .take_while(|t| *t <= now)
                .count()
                .saturating_sub(1) // the "next" tick isn't missed, it's upcoming
        }
        JobSchedule::Interval(d) => {
            let elapsed = (now - last_run).num_seconds().max(0) as u64;
            let interval_secs = d.as_secs();
            if interval_secs == 0 {
                return 0;
            }
            (elapsed / interval_secs).saturating_sub(1) as usize
        }
    }
}
