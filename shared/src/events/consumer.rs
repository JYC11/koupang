use std::sync::Arc;
use std::time::{Duration, Instant};

use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Header, Message, OwnedHeaders};
use rdkafka::producer::{FutureProducer, FutureRecord};
use tokio_util::sync::CancellationToken;

use crate::config::consumer_config::ConsumerConfig;
use crate::config::kafka_config::KafkaConfig;
use crate::db::PgPool;
use crate::errors::AppError;
use crate::events::metrics::ConsumerMetricsCollector;
use crate::events::{EventEnvelope, KafkaAdmin, TopicSpec};
use crate::outbox::{cleanup_processed_events, is_event_processed, mark_event_processed};

// ── Handler error ───────────────────────────────────────────────────

/// Error returned by an `EventHandler` to signal whether the failure is retryable.
pub enum HandlerError {
    /// Transient failure — the consumer will retry up to `max_retries`.
    Transient(Box<dyn std::error::Error + Send + Sync>),
    /// Permanent failure — skip retries, send directly to DLQ.
    Permanent(Box<dyn std::error::Error + Send + Sync>),
}

impl HandlerError {
    pub fn transient(msg: impl Into<String>) -> Self {
        Self::Transient(msg.into().into())
    }

    pub fn permanent(msg: impl Into<String>) -> Self {
        Self::Permanent(msg.into().into())
    }

    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Transient(_))
    }
}

impl From<AppError> for HandlerError {
    fn from(e: AppError) -> Self {
        Self::Transient(Box::new(e))
    }
}

impl std::fmt::Display for HandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient(e) => write!(f, "transient: {e}"),
            Self::Permanent(e) => write!(f, "permanent: {e}"),
        }
    }
}

// ── Event handler trait ─────────────────────────────────────────────

/// Handler for processing consumed events.
///
/// Each consuming service implements this trait with its own event handling logic.
/// The handler receives a mutable reference to a database connection within a
/// transaction — any writes it performs are committed atomically with the
/// `processed_events` idempotency marker.
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    async fn handle(
        &self,
        envelope: &EventEnvelope,
        tx: &mut sqlx::PgConnection,
    ) -> Result<(), HandlerError>;
}

// ── Kafka event consumer ────────────────────────────────────────────

/// Kafka consumer with transactional idempotency and dead-letter queue support.
///
/// Processing flow per message:
/// ```text
/// deserialize → EventEnvelope
///   ├─ deser failure → DLQ (raw bytes) → commit offset
///   └─ success → retry loop (up to max_retries):
///       BEGIN TX
///       ├─ already processed? → COMMIT → commit offset (skip)
///       └─ handler.handle(envelope, &mut tx)
///           ├─ Ok → mark_event_processed(tx) → COMMIT → commit offset
///           └─ Transient(err) → ROLLBACK → backoff → retry
///       After exhausting retries or on Permanent(err):
///           → DLQ → commit offset (or no-commit if DLQ fails)
/// ```
pub struct KafkaEventConsumer {
    consumer: StreamConsumer,
    handler: Arc<dyn EventHandler>,
    pool: PgPool,
    producer: FutureProducer,
    config: ConsumerConfig,
    kafka_config: KafkaConfig,
    metrics: Arc<ConsumerMetricsCollector>,
}

impl KafkaEventConsumer {
    pub fn new(
        kafka_config: &KafkaConfig,
        config: ConsumerConfig,
        handler: Arc<dyn EventHandler>,
        pool: PgPool,
    ) -> Result<Self, AppError> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &kafka_config.brokers)
            .set("group.id", &config.group_id)
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", "earliest")
            .set(
                "session.timeout.ms",
                config.session_timeout.as_millis().to_string(),
            )
            .create()
            .map_err(|e| AppError::InternalServerError(format!("Kafka consumer init: {e}")))?;

        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &kafka_config.brokers)
            .set("message.timeout.ms", "10000")
            .create()
            .map_err(|e| AppError::InternalServerError(format!("Kafka DLQ producer init: {e}")))?;

        Ok(Self {
            consumer,
            handler,
            pool,
            producer,
            config,
            kafka_config: KafkaConfig::from_brokers(&kafka_config.brokers),
            metrics: Arc::new(ConsumerMetricsCollector::new()),
        })
    }

    /// Returns a handle to the consumer's metrics collector.
    ///
    /// Call this before [`run`](Self::run) to keep a reference for monitoring endpoints.
    pub fn metrics(&self) -> Arc<ConsumerMetricsCollector> {
        Arc::clone(&self.metrics)
    }

    /// Start consuming. Runs until the cancellation token is triggered.
    pub async fn run(self, shutdown: CancellationToken) {
        let consumer = Arc::new(self);

        // Auto-create DLQ topics
        if consumer.config.auto_create_dlq_topics
            && let Err(e) = consumer.ensure_dlq_topics().await
        {
            tracing::error!(error = %e, "Failed to auto-create DLQ topics, continuing anyway");
        }

        // Subscribe
        let topic_refs: Vec<&str> = consumer.config.topics.iter().map(|s| s.as_str()).collect();
        if let Err(e) = consumer.consumer.subscribe(&topic_refs) {
            tracing::error!(error = %e, "Failed to subscribe to topics");
            return;
        }

        tracing::info!(
            group_id = %consumer.config.group_id,
            topics = ?consumer.config.topics,
            "Kafka consumer started"
        );

        let message_handle = {
            let c = Arc::clone(&consumer);
            let s = shutdown.clone();
            tokio::spawn(async move { Self::message_loop(c, s).await })
        };

        let cleanup_handle = {
            let c = Arc::clone(&consumer);
            let s = shutdown.clone();
            tokio::spawn(async move { Self::cleanup_loop(c, s).await })
        };

        let _ = tokio::join!(message_handle, cleanup_handle);
        tracing::info!("Kafka consumer shut down gracefully");
    }

    // ── Message loop ────────────────────────────────────────────────

    async fn message_loop(consumer: Arc<Self>, shutdown: CancellationToken) {
        loop {
            tokio::select! {
                biased;

                _ = shutdown.cancelled() => {
                    tracing::info!("Consumer message loop: shutdown received");
                    return;
                }

                msg_result = consumer.consumer.recv() => {
                    match msg_result {
                        Ok(msg) => {
                            consumer.process_message(&msg, &shutdown).await;
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Kafka consumer recv error");
                            tokio::select! {
                                biased;
                                _ = shutdown.cancelled() => return,
                                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Per-message processing ──────────────────────────────────────

    async fn process_message(&self, msg: &BorrowedMessage<'_>, shutdown: &CancellationToken) {
        let started_at = Instant::now();
        let topic = msg.topic().to_string();
        let raw_payload = msg.payload().unwrap_or_default();

        // Step 1: Deserialize
        let envelope = match serde_json::from_slice::<EventEnvelope>(raw_payload) {
            Ok(env) => env,
            Err(e) => {
                tracing::warn!(
                    topic = %topic,
                    error = %e,
                    "Failed to deserialize event, sending raw bytes to DLQ"
                );
                match self
                    .publish_raw_to_dlq(&topic, raw_payload, &e.to_string())
                    .await
                {
                    Ok(()) => {
                        self.metrics.record_deser_failed(started_at);
                        self.commit_offset(msg);
                    }
                    Err(dlq_err) => {
                        self.metrics.record_dlq_failed(started_at);
                        tracing::error!(
                            error = %dlq_err,
                            "DLQ publish failed for deser failure, not committing offset"
                        );
                    }
                }
                return;
            }
        };

        // Step 2: Process with retry
        match self
            .process_with_retry(&envelope, &topic, shutdown, started_at)
            .await
        {
            ProcessResult::Success | ProcessResult::Skipped | ProcessResult::SentToDlq => {
                self.commit_offset(msg);
            }
            ProcessResult::DlqFailed | ProcessResult::DbError => {
                // Do NOT commit — message will be redelivered on next poll
            }
        }
    }

    /// Retry loop: each attempt gets its own database transaction.
    async fn process_with_retry(
        &self,
        envelope: &EventEnvelope,
        topic: &str,
        shutdown: &CancellationToken,
        started_at: Instant,
    ) -> ProcessResult {
        let event_id = envelope.metadata.event_id;
        let event_type = envelope.metadata.event_type.to_string();
        let source_service = envelope.metadata.source_service.to_string();
        let mut last_error = String::new();

        for attempt in 0..=self.config.max_retries {
            // Begin transaction
            let mut tx = match self.pool.begin().await {
                Ok(tx) => tx,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to begin transaction");
                    self.metrics.record_db_error(started_at);
                    return ProcessResult::DbError;
                }
            };

            // Idempotency check
            match is_event_processed(&mut *tx, event_id).await {
                Ok(true) => {
                    if let Err(e) = tx.commit().await {
                        tracing::error!(error = %e, "Failed to commit skip-transaction");
                        self.metrics.record_db_error(started_at);
                        return ProcessResult::DbError;
                    }
                    tracing::debug!(event_id = %event_id, "Event already processed, skipping");
                    self.metrics.record_skipped(started_at);
                    return ProcessResult::Skipped;
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::error!(error = %e, "Idempotency check failed");
                    self.metrics.record_db_error(started_at);
                    return ProcessResult::DbError;
                }
            }

            // Call handler
            match self.handler.handle(envelope, &mut tx).await {
                Ok(()) => {
                    // Mark processed in same transaction
                    if let Err(e) =
                        mark_event_processed(&mut *tx, event_id, &event_type, &source_service).await
                    {
                        tracing::error!(error = %e, "Failed to mark event processed");
                        self.metrics.record_db_error(started_at);
                        return ProcessResult::DbError;
                    }
                    if let Err(e) = tx.commit().await {
                        tracing::error!(error = %e, "Failed to commit transaction");
                        self.metrics.record_db_error(started_at);
                        return ProcessResult::DbError;
                    }
                    tracing::debug!(
                        event_id = %event_id,
                        event_type = %event_type,
                        "Event processed successfully"
                    );
                    self.metrics.record_success(started_at);
                    return ProcessResult::Success;
                }
                Err(HandlerError::Permanent(e)) => {
                    last_error = e.to_string();
                    drop(tx); // ROLLBACK
                    tracing::warn!(
                        event_id = %event_id,
                        error = %last_error,
                        "Permanent handler error, sending to DLQ"
                    );
                    let result = self
                        .publish_envelope_to_dlq(topic, envelope, &last_error, 0)
                        .await;
                    match result {
                        ProcessResult::SentToDlq => self.metrics.record_dlq(started_at),
                        _ => self.metrics.record_dlq_failed(started_at),
                    }
                    return result;
                }
                Err(HandlerError::Transient(e)) => {
                    last_error = e.to_string();
                    drop(tx); // ROLLBACK

                    if attempt >= self.config.max_retries {
                        break; // Exhausted retries → DLQ below
                    }

                    self.metrics.record_retry();
                    tracing::warn!(
                        event_id = %event_id,
                        error = %last_error,
                        attempt = attempt + 1,
                        max_retries = self.config.max_retries,
                        "Transient handler error, retrying"
                    );

                    // Backoff between retries, checking shutdown
                    let delay = self.calculate_backoff(attempt);
                    tokio::select! {
                        biased;
                        _ = shutdown.cancelled() => {
                            tracing::info!("Shutdown during retry backoff, sending to DLQ");
                            let result = self
                                .publish_envelope_to_dlq(
                                    topic,
                                    envelope,
                                    &format!("shutdown during backoff: {last_error}"),
                                    attempt + 1,
                                )
                                .await;
                            match result {
                                ProcessResult::SentToDlq => self.metrics.record_dlq(started_at),
                                _ => self.metrics.record_dlq_failed(started_at),
                            }
                            return result;
                        }
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }

        // Exhausted all retries
        tracing::warn!(
            event_id = %envelope.metadata.event_id,
            retries = self.config.max_retries,
            error = %last_error,
            "Handler exhausted retries, sending to DLQ"
        );
        let result = self
            .publish_envelope_to_dlq(topic, envelope, &last_error, self.config.max_retries)
            .await;
        match result {
            ProcessResult::SentToDlq => self.metrics.record_dlq(started_at),
            _ => self.metrics.record_dlq_failed(started_at),
        }
        result
    }

    // ── DLQ publishing ──────────────────────────────────────────────

    async fn publish_envelope_to_dlq(
        &self,
        original_topic: &str,
        envelope: &EventEnvelope,
        error: &str,
        retry_count: u32,
    ) -> ProcessResult {
        let dlq_topic = self.dlq_topic_for(original_topic);
        let payload = match serde_json::to_string(envelope) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize envelope for DLQ");
                return ProcessResult::DlqFailed;
            }
        };

        let key = envelope.partition_key();
        let retry_count_str = retry_count.to_string();
        let timestamp = chrono::Utc::now().to_rfc3339();

        let headers = OwnedHeaders::new_with_capacity(5)
            .insert(Header {
                key: "dlq_reason",
                value: Some(error),
            })
            .insert(Header {
                key: "dlq_retry_count",
                value: Some(&retry_count_str),
            })
            .insert(Header {
                key: "dlq_original_topic",
                value: Some(original_topic),
            })
            .insert(Header {
                key: "dlq_timestamp",
                value: Some(&timestamp),
            })
            .insert(Header {
                key: "dlq_consumer_group",
                value: Some(&self.config.group_id),
            });

        let record = FutureRecord::to(&dlq_topic)
            .key(&key)
            .payload(&payload)
            .headers(headers);

        match self.producer.send(record, Duration::from_secs(10)).await {
            Ok(_) => {
                tracing::warn!(
                    dlq_topic = %dlq_topic,
                    event_id = %envelope.metadata.event_id,
                    error = %error,
                    "Event sent to DLQ"
                );
                ProcessResult::SentToDlq
            }
            Err((e, _)) => {
                tracing::error!(
                    dlq_topic = %dlq_topic,
                    error = %e,
                    "Failed to publish to DLQ, not committing offset"
                );
                ProcessResult::DlqFailed
            }
        }
    }

    async fn publish_raw_to_dlq(
        &self,
        original_topic: &str,
        raw_payload: &[u8],
        error: &str,
    ) -> Result<(), AppError> {
        let dlq_topic = self.dlq_topic_for(original_topic);
        let timestamp = chrono::Utc::now().to_rfc3339();

        let headers = OwnedHeaders::new_with_capacity(5)
            .insert(Header {
                key: "dlq_reason",
                value: Some(error),
            })
            .insert(Header {
                key: "dlq_retry_count",
                value: Some("0"),
            })
            .insert(Header {
                key: "dlq_original_topic",
                value: Some(original_topic),
            })
            .insert(Header {
                key: "dlq_timestamp",
                value: Some(&timestamp),
            })
            .insert(Header {
                key: "dlq_consumer_group",
                value: Some(&self.config.group_id),
            });

        let record = FutureRecord::<(), [u8]>::to(&dlq_topic)
            .payload(raw_payload)
            .headers(headers);

        self.producer
            .send(record, Duration::from_secs(10))
            .await
            .map_err(|(e, _)| AppError::InternalServerError(format!("DLQ publish: {e}")))?;

        tracing::warn!(
            dlq_topic = %dlq_topic,
            error = %error,
            "Raw bytes sent to DLQ (deserialization failure)"
        );
        Ok(())
    }

    // ── Helpers ─────────────────────────────────────────────────────

    fn dlq_topic_for(&self, source_topic: &str) -> String {
        match &self.config.dlq_topic_override {
            Some(t) => t.clone(),
            None => format!("{source_topic}.dlq"),
        }
    }

    fn commit_offset(&self, msg: &BorrowedMessage<'_>) {
        if let Err(e) = self.consumer.commit_message(msg, CommitMode::Async) {
            tracing::error!(error = %e, "Failed to commit Kafka offset");
        }
    }

    /// Exponential backoff: base × 2^attempt, capped at retry_max_delay.
    fn calculate_backoff(&self, attempt: u32) -> Duration {
        let base_ms = self.config.retry_base_delay.as_millis() as u64;
        let delay_ms = base_ms.saturating_mul(1u64 << attempt.min(10));
        let max_ms = self.config.retry_max_delay.as_millis() as u64;
        Duration::from_millis(delay_ms.min(max_ms))
    }

    async fn ensure_dlq_topics(&self) -> Result<(), AppError> {
        let admin = KafkaAdmin::new(&self.kafka_config)?;
        let dlq_specs: Vec<TopicSpec> = self
            .config
            .topics
            .iter()
            .map(|t| TopicSpec::new(self.dlq_topic_for(t), 1, 1))
            .collect();
        admin.ensure_topics(&dlq_specs).await
    }

    // ── Cleanup loop ────────────────────────────────────────────────

    async fn cleanup_loop(consumer: Arc<Self>, shutdown: CancellationToken) {
        loop {
            tokio::select! {
                biased;

                _ = shutdown.cancelled() => {
                    tracing::info!("Consumer cleanup loop: shutdown received");
                    return;
                }

                _ = tokio::time::sleep(consumer.config.processed_events_cleanup_interval) => {}
            }

            let max_age_secs = consumer.config.processed_events_max_age.as_secs() as i64;
            match cleanup_processed_events(&consumer.pool, max_age_secs).await {
                Ok(0) => {}
                Ok(n) => tracing::info!(count = n, "Cleaned up old processed events"),
                Err(e) => tracing::error!(error = %e, "Failed to cleanup processed events"),
            }
        }
    }
}

/// Internal result of processing a single message.
enum ProcessResult {
    /// Handler succeeded, event marked as processed.
    Success,
    /// Event was already processed (idempotency skip).
    Skipped,
    /// Event sent to DLQ after handler failure.
    SentToDlq,
    /// DLQ publish itself failed — do NOT commit offset.
    DlqFailed,
    /// Database error — do NOT commit offset.
    DbError,
}

// ── Unit tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculate_backoff_exponential() {
        let config = ConsumerConfig::new("g", vec![]);
        let consumer_config = &config;

        // Create a helper to test backoff without a full consumer
        let base_ms = consumer_config.retry_base_delay.as_millis() as u64;
        let max_ms = consumer_config.retry_max_delay.as_millis() as u64;

        let backoff = |attempt: u32| -> Duration {
            let delay_ms = base_ms.saturating_mul(1u64 << attempt.min(10));
            Duration::from_millis(delay_ms.min(max_ms))
        };

        // base=1s: 1s, 2s, 4s, 8s, 16s, 30s (capped)
        assert_eq!(backoff(0), Duration::from_secs(1));
        assert_eq!(backoff(1), Duration::from_secs(2));
        assert_eq!(backoff(2), Duration::from_secs(4));
        assert_eq!(backoff(3), Duration::from_secs(8));
        assert_eq!(backoff(4), Duration::from_secs(16));
        assert_eq!(backoff(5), Duration::from_secs(30)); // capped at max
        assert_eq!(backoff(10), Duration::from_secs(30)); // still capped
    }

    #[test]
    fn dlq_topic_naming() {
        // Default: {topic}.dlq
        let config = ConsumerConfig::new("g", vec!["order.events".into()]);
        assert_eq!(config.dlq_topic_override.as_deref(), None::<&str>,);
        // Test the logic directly
        let dlq = match &config.dlq_topic_override {
            Some(t) => t.clone(),
            None => format!("{}.dlq", "order.events"),
        };
        assert_eq!(dlq, "order.events.dlq");

        // Override
        let mut config2 = ConsumerConfig::new("g", vec!["order.events".into()]);
        config2.dlq_topic_override = Some("all-dlq".into());
        let dlq2 = match &config2.dlq_topic_override {
            Some(t) => t.clone(),
            None => format!("{}.dlq", "order.events"),
        };
        assert_eq!(dlq2, "all-dlq");
    }

    #[test]
    fn handler_error_display() {
        let t = HandlerError::transient("connection refused");
        assert_eq!(t.to_string(), "transient: connection refused");
        assert!(t.is_transient());

        let p = HandlerError::permanent("invalid payload");
        assert_eq!(p.to_string(), "permanent: invalid payload");
        assert!(!p.is_transient());
    }

    #[test]
    fn handler_error_from_app_error() {
        let app_err = AppError::InternalServerError("db down".into());
        let handler_err: HandlerError = app_err.into();
        assert!(handler_err.is_transient());
    }
}
