use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde::Serialize;

/// Thread-safe in-memory counters for consumer activity.
///
/// Shared between the consumer (writer) and monitoring endpoints (reader)
/// via `Arc<ConsumerMetricsCollector>`. Call [`snapshot`](Self::snapshot)
/// to get a point-in-time copy of all counters.
#[derive(Debug, Default)]
pub struct ConsumerMetricsCollector {
    events_processed: AtomicU64,
    events_skipped: AtomicU64,
    events_sent_to_dlq: AtomicU64,
    events_dlq_failed: AtomicU64,
    events_db_error: AtomicU64,
    events_deser_failed: AtomicU64,
    events_retried: AtomicU64,
    /// Cumulative processing time in microseconds (for computing average).
    processing_duration_us: AtomicU64,
}

impl ConsumerMetricsCollector {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Writers (called by consumer internals) ──────────────────────

    pub(crate) fn record_success(&self, started_at: Instant) {
        self.events_processed.fetch_add(1, Ordering::Relaxed);
        self.record_duration(started_at);
    }

    pub(crate) fn record_skipped(&self, started_at: Instant) {
        self.events_skipped.fetch_add(1, Ordering::Relaxed);
        self.record_duration(started_at);
    }

    pub(crate) fn record_dlq(&self, started_at: Instant) {
        self.events_sent_to_dlq.fetch_add(1, Ordering::Relaxed);
        self.record_duration(started_at);
    }

    pub(crate) fn record_dlq_failed(&self, started_at: Instant) {
        self.events_dlq_failed.fetch_add(1, Ordering::Relaxed);
        self.record_duration(started_at);
    }

    pub(crate) fn record_db_error(&self, started_at: Instant) {
        self.events_db_error.fetch_add(1, Ordering::Relaxed);
        self.record_duration(started_at);
    }

    pub(crate) fn record_deser_failed(&self, started_at: Instant) {
        self.events_deser_failed.fetch_add(1, Ordering::Relaxed);
        self.record_duration(started_at);
    }

    pub(crate) fn record_retry(&self) {
        self.events_retried.fetch_add(1, Ordering::Relaxed);
    }

    fn record_duration(&self, started_at: Instant) {
        let us = started_at.elapsed().as_micros() as u64;
        self.processing_duration_us.fetch_add(us, Ordering::Relaxed);
    }

    // ── Reader ──────────────────────────────────────────────────────

    /// Point-in-time snapshot of all counters.
    pub fn snapshot(&self) -> ConsumerMetrics {
        let events_processed = self.events_processed.load(Ordering::Relaxed);
        let events_skipped = self.events_skipped.load(Ordering::Relaxed);
        let events_sent_to_dlq = self.events_sent_to_dlq.load(Ordering::Relaxed);
        let events_dlq_failed = self.events_dlq_failed.load(Ordering::Relaxed);
        let events_db_error = self.events_db_error.load(Ordering::Relaxed);
        let events_deser_failed = self.events_deser_failed.load(Ordering::Relaxed);
        let events_retried = self.events_retried.load(Ordering::Relaxed);
        let total_duration_us = self.processing_duration_us.load(Ordering::Relaxed);

        let total_events = events_processed
            + events_skipped
            + events_sent_to_dlq
            + events_dlq_failed
            + events_db_error
            + events_deser_failed;

        let avg_processing_duration_ms = if total_events > 0 {
            (total_duration_us as f64 / total_events as f64) / 1000.0
        } else {
            0.0
        };

        ConsumerMetrics {
            events_processed,
            events_skipped,
            events_sent_to_dlq,
            events_dlq_failed,
            events_db_error,
            events_deser_failed,
            events_retried,
            total_events,
            avg_processing_duration_ms,
        }
    }
}

/// Point-in-time snapshot of consumer metrics.
#[derive(Debug, Clone, Serialize)]
pub struct ConsumerMetrics {
    /// Events successfully processed by the handler.
    pub events_processed: u64,
    /// Events skipped (already processed — idempotency dedup).
    pub events_skipped: u64,
    /// Events sent to the dead-letter queue.
    pub events_sent_to_dlq: u64,
    /// Events where DLQ publish itself failed (will be redelivered).
    pub events_dlq_failed: u64,
    /// Events that hit a database error (will be redelivered).
    pub events_db_error: u64,
    /// Events that failed deserialization (sent to DLQ as raw bytes).
    pub events_deser_failed: u64,
    /// Total number of transient-error retries across all events.
    pub events_retried: u64,
    /// Sum of all outcome counters.
    pub total_events: u64,
    /// Average wall-clock time per event (across all outcomes), in milliseconds.
    pub avg_processing_duration_ms: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn snapshot_starts_at_zero() {
        let collector = ConsumerMetricsCollector::new();
        let snap = collector.snapshot();
        assert_eq!(snap.events_processed, 0);
        assert_eq!(snap.total_events, 0);
        assert_eq!(snap.avg_processing_duration_ms, 0.0);
    }

    #[test]
    fn snapshot_reflects_recorded_events() {
        let collector = ConsumerMetricsCollector::new();
        let now = Instant::now();

        collector.record_success(now);
        collector.record_success(now);
        collector.record_skipped(now);
        collector.record_dlq(now);
        collector.record_retry();
        collector.record_retry();

        let snap = collector.snapshot();
        assert_eq!(snap.events_processed, 2);
        assert_eq!(snap.events_skipped, 1);
        assert_eq!(snap.events_sent_to_dlq, 1);
        assert_eq!(snap.events_retried, 2);
        assert_eq!(snap.total_events, 4);
    }

    #[test]
    fn avg_duration_computed_correctly() {
        let collector = ConsumerMetricsCollector::new();

        // Simulate two events with known durations
        // We can't control Instant::now() precisely, so test the math via atomics directly
        collector.events_processed.store(2, Ordering::Relaxed);
        // 10ms = 10_000us total across 2 events → avg 5ms
        collector
            .processing_duration_us
            .store(10_000, Ordering::Relaxed);

        let snap = collector.snapshot();
        assert_eq!(snap.total_events, 2);
        assert!((snap.avg_processing_duration_ms - 5.0).abs() < 0.01);
    }

    #[test]
    fn record_duration_adds_elapsed_time() {
        let collector = ConsumerMetricsCollector::new();
        let before = Instant::now();
        std::thread::sleep(Duration::from_millis(5));
        collector.record_success(before);

        let us = collector.processing_duration_us.load(Ordering::Relaxed);
        // Should be at least 4ms worth of microseconds (allowing for timing jitter)
        assert!(us >= 4_000, "expected >= 4000us, got {us}us");
    }
}
