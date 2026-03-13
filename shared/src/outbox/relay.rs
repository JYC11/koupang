use std::sync::Arc;

use sqlx::postgres::PgListener;
use tokio_util::sync::CancellationToken;

use crate::config::relay_config::RelayConfig;
use crate::db::PgPool;
use crate::errors::AppError;
use crate::events::{EventEnvelope, EventPublisher};
use crate::outbox::repository::{
    claim_batch, cleanup_published, delete_published, mark_published, mark_retry_or_failed,
    release_stale_locks,
};
use crate::outbox::types::{LogFailureEscalation, OutboxEvent, RelayHeartbeat};

/// Background relay that reads pending events from the outbox table and publishes them to Kafka.
///
/// Runs three concurrent loops:
/// 1. **Relay loop** — claims batches of pending events, publishes them, and marks them done.
///    Woken by PgListener NOTIFY or falls back to `poll_interval` polling.
/// 2. **Stale lock loop** — periodically frees events locked by crashed relay instances.
/// 3. **Cleanup loop** — periodically deletes old published events to prevent table bloat.
pub struct OutboxRelay {
    pool: PgPool,
    publisher: Arc<dyn EventPublisher>,
    config: RelayConfig,
    heartbeat: Arc<RelayHeartbeat>,
}

impl OutboxRelay {
    pub fn new(pool: PgPool, publisher: Arc<dyn EventPublisher>, config: RelayConfig) -> Self {
        Self {
            pool,
            publisher,
            config,
            heartbeat: Arc::new(RelayHeartbeat::new()),
        }
    }

    /// Returns a handle to the relay's heartbeat tracker.
    ///
    /// Call this before `run()` and pass the `Arc` to your health/metrics
    /// endpoint. The relay updates the heartbeat on every loop iteration.
    pub fn heartbeat(&self) -> Arc<RelayHeartbeat> {
        Arc::clone(&self.heartbeat)
    }

    /// Start the relay. Consumes self and runs until the cancellation token is triggered.
    pub async fn run(self, shutdown: CancellationToken) {
        let relay = Arc::new(self);

        let relay_handle = {
            let r = Arc::clone(&relay);
            let s = shutdown.clone();
            tokio::spawn(async move { Self::relay_loop(r, s).await })
        };

        let stale_handle = {
            let r = Arc::clone(&relay);
            let s = shutdown.clone();
            tokio::spawn(async move { Self::stale_lock_loop(r, s).await })
        };

        let cleanup_handle = {
            let r = Arc::clone(&relay);
            let s = shutdown.clone();
            tokio::spawn(async move { Self::cleanup_loop(r, s).await })
        };

        // Wait for all loops to finish (they exit when shutdown is cancelled)
        let _ = tokio::join!(relay_handle, stale_handle, cleanup_handle);

        tracing::info!("Outbox relay shut down gracefully");
    }

    // ── Main relay loop ────────────────────────────────────────────

    async fn relay_loop(relay: Arc<Self>, shutdown: CancellationToken) {
        let mut listener = Self::connect_listener(&relay.pool).await;

        loop {
            // Wait for a wake signal: PG notification, poll interval, or shutdown
            tokio::select! {
                biased;

                _ = shutdown.cancelled() => {
                    tracing::info!("Relay loop: shutdown signal received");
                    return;
                }

                // PgListener notification — an event was just inserted
                notification = async {
                    match listener.as_mut() {
                        Some(l) => l.recv().await.map(Some),
                        None => std::future::pending().await,
                    }
                } => {
                    match notification {
                        Ok(Some(_)) => {
                            tracing::debug!("Relay woken by PG notification");
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "PgListener error, attempting reconnect");
                            listener = Self::connect_listener(&relay.pool).await;
                        }
                        _ => {}
                    }
                }

                // Fallback polling interval — also attempt PgListener reconnect if disconnected
                _ = tokio::time::sleep(relay.config.poll_interval) => {
                    tracing::trace!("Relay woken by poll interval");
                    if listener.is_none() {
                        listener = Self::connect_listener(&relay.pool).await;
                    }
                }
            }

            relay.heartbeat.beat();
            relay.process_pending(&shutdown).await;
        }
    }

    /// Drain all available pending events in a loop until a batch returns empty.
    async fn process_pending(&self, shutdown: &CancellationToken) {
        loop {
            if shutdown.is_cancelled() {
                return;
            }
            match self.process_batch().await {
                Ok(0) => return,
                Ok(_) => continue,
                Err(e) => {
                    tracing::error!(error = %e, "Error processing outbox batch, backing off 1s");
                    tokio::select! {
                        biased;
                        _ = shutdown.cancelled() => {}
                        _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                    }
                    return;
                }
            }
        }
    }

    /// Claim a batch, publish each event, and update status. Returns the number of events processed.
    ///
    /// Individual event DB updates (mark_published, mark_retry_or_failed) do NOT abort the batch.
    /// On DB error for a single event, the error is logged and the loop continues. The event
    /// remains locked and will be freed by the stale lock recovery loop.
    async fn process_batch(&self) -> Result<usize, AppError> {
        let events =
            claim_batch(&self.pool, self.config.batch_size, &self.config.instance_id).await?;

        if events.is_empty() {
            return Ok(0);
        }

        let count = events.len();
        tracing::debug!(count, "Processing outbox batch");

        for mut event in events {
            // Take ownership of payload to avoid cloning the (potentially large) JSON tree.
            // The event struct remains intact with Value::Null in the payload field,
            // which is fine since we only use metadata fields after this point.
            let payload = std::mem::take(&mut event.payload);

            match self.publish_payload(&event.topic, payload).await {
                Ok(()) => {
                    let update_result = if self.config.delete_on_publish {
                        delete_published(&self.pool, event.id).await
                    } else {
                        mark_published(&self.pool, event.id).await
                    };
                    if let Err(db_err) = update_result {
                        tracing::error!(
                            event_id = %event.event_id,
                            error = %db_err,
                            "Failed to mark outbox event as published, stale lock recovery will free it"
                        );
                        continue;
                    }
                    tracing::debug!(
                        event_id = %event.event_id,
                        event_type = %event.event_type,
                        "Published outbox event"
                    );
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    tracing::warn!(
                        event_id = %event.event_id,
                        error = %error_msg,
                        retry_count = event.retry_count,
                        max_retries = event.max_retries,
                        "Failed to publish outbox event"
                    );

                    match mark_retry_or_failed(&self.pool, event.id, &error_msg).await {
                        Ok(()) => {
                            // Escalate only AFTER the status transition succeeds.
                            // This prevents spurious escalations when the DB update fails
                            // (the event stays pending and would be re-escalated on next attempt).
                            if event.retry_count + 1 >= event.max_retries {
                                self.escalate_failure(&event).await;
                            }
                        }
                        Err(db_err) => {
                            tracing::error!(
                                event_id = %event.event_id,
                                error = %db_err,
                                "Failed to update retry status, stale lock recovery will free it"
                            );
                        }
                    }
                }
            }
        }

        Ok(count)
    }

    /// Deserialize an outbox payload into an EventEnvelope and publish to Kafka.
    /// Takes ownership of the payload Value to avoid cloning.
    async fn publish_payload(
        &self,
        topic: &str,
        payload: serde_json::Value,
    ) -> Result<(), AppError> {
        let envelope: EventEnvelope = serde_json::from_value(payload).map_err(|e| {
            AppError::InternalServerError(format!("Deserialize outbox payload: {e}"))
        })?;
        self.publisher.publish(topic, &envelope).await
    }

    /// Invoke the configured failure escalation handler (or the default log handler).
    async fn escalate_failure(&self, event: &OutboxEvent) {
        let handler: &dyn crate::outbox::FailureEscalation =
            match self.config.failure_escalation.as_ref() {
                Some(h) => h.as_ref(),
                None => &LogFailureEscalation,
            };

        if let Err(e) = handler.on_permanent_failure(event).await {
            tracing::error!(
                event_id = %event.event_id,
                error = %e,
                "Failure escalation handler itself failed"
            );
        }
    }

    // ── Stale lock recovery loop ───────────────────────────────────

    async fn stale_lock_loop(relay: Arc<Self>, shutdown: CancellationToken) {
        let timeout_secs = relay.config.stale_lock_timeout.as_secs() as i64;

        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => {
                    tracing::info!("Stale lock loop: shutdown signal received");
                    return;
                }
                _ = tokio::time::sleep(relay.config.stale_lock_check_interval) => {}
            }

            match release_stale_locks(&relay.pool, timeout_secs).await {
                Ok(0) => {}
                Ok(n) => tracing::info!(count = n, "Released stale outbox locks"),
                Err(e) => tracing::error!(error = %e, "Failed to release stale locks"),
            }
        }
    }

    // ── Cleanup loop ───────────────────────────────────────────────

    async fn cleanup_loop(relay: Arc<Self>, shutdown: CancellationToken) {
        let max_age_secs = relay.config.cleanup_max_age.as_secs() as i64;

        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => {
                    tracing::info!("Cleanup loop: shutdown signal received");
                    return;
                }
                _ = tokio::time::sleep(relay.config.cleanup_interval) => {}
            }

            // Drain in batches of 1000 to avoid long transactions with 100K+ rows.
            let mut total = 0u64;
            loop {
                if shutdown.is_cancelled() {
                    break;
                }
                match cleanup_published(&relay.pool, max_age_secs).await {
                    Ok(0) => break,
                    Ok(n) => total += n,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to cleanup published events");
                        break;
                    }
                }
            }
            if total > 0 {
                tracing::info!(count = total, "Cleaned up old published outbox events");
            }
        }
    }

    // ── PgListener setup ───────────────────────────────────────────

    async fn connect_listener(pool: &PgPool) -> Option<PgListener> {
        match PgListener::connect_with(pool).await {
            Ok(mut listener) => match listener.listen("outbox_events").await {
                Ok(()) => {
                    tracing::info!("PgListener connected, listening on 'outbox_events' channel");
                    Some(listener)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to listen on outbox_events channel, using poll-only mode");
                    None
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "Failed to connect PgListener, using poll-only mode");
                None
            }
        }
    }
}
