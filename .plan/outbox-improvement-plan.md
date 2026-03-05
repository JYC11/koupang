# Outbox Implementation: Edge Cases & Improvement Plan

Assessment of the transactional outbox (shared/src/outbox/) for production readiness.

## Overall Assessment

Well-structured with strong test coverage (8 relay tests + 9 Kafka pipeline tests + repository tests). Per-aggregate ordering via `DISTINCT ON`, `FOR UPDATE SKIP LOCKED`, DB-enforced state machine, and LISTEN/NOTIFY are all solid production patterns. Issues below are mostly failure-mode edges, not the happy path.

## Issues

### 1. Batch abort leaves remaining events locked up to 90s

**Severity: High** | **Effort: Small**

`relay.rs:136-187` — The loop claims a batch, then processes events one by one. If `mark_published` or `mark_retry_or_failed` fails for event N, the `?` propagates up and the loop returns `Err`. Events N+1, N+2, ... are still locked by this relay instance. They won't be processed until the stale lock loop frees them (default: 60s timeout, checked every 30s = up to 90s of latency).

**Fix options:**
- Continue processing remaining events in the batch even if one DB update fails (log the error, skip that event)
- Or release locks for unprocessed events on error: `UPDATE outbox_events SET locked_by = NULL WHERE locked_by = $1 AND status = 'pending'`

### 2. Escalation fires before status transition

**Severity: Medium** | **Effort: Trivial (swap two lines)**

`relay.rs:177-181`:
```rust
if event.retry_count + 1 >= event.max_retries {
    self.escalate_failure(&event).await;  // fires BEFORE mark
}
mark_retry_or_failed(&self.pool, event.id, &error_msg).await?;
```

The escalation handler receives an `OutboxEvent` with `status: Pending` even though the event is about to become `Failed`. If `mark_retry_or_failed` then fails, the escalation fired but the event didn't actually transition — it will be retried and escalated again.

**Fix:** Call `mark_retry_or_failed` first, then escalate.

### 3. No payload size guard

**Severity: Medium** | **Effort: Small**

No upper bound on payload size. A 10MB payload would:
- Consume significant memory during `serde_json::from_value` deserialization
- Potentially exceed Kafka's `message.max.bytes` (default 1MB) and fail
- The `std::mem::take` optimization at `relay.rs:151` avoids a clone but the allocation still exists

**Fix:** Validate payload size at `insert_outbox_event` time (e.g., reject > 900KB to stay under Kafka's 1MB default), or set `message.max.bytes` explicitly in the producer config.

### 4. Producer timeout too aggressive

**Severity: Medium** | **Effort: Config change**

`producer.rs:21` sets `message.timeout.ms = 5000`. Under broker pressure, 5s can expire before acknowledgment. Combined with the relay's backoff starting at 2s (`POW(2, 1)`), the first few retries cycle rapidly (2s wait + 5s timeout = 7s cycles), potentially hammering a stressed broker.

**Fix:** Consider `message.timeout.ms = 30000` (rdkafka default) to let the producer's internal retry handle transient broker issues before surfacing the error to the relay.

### 5. `claim_batch` CTE may slow with many aggregates

**Severity: Medium** | **Effort: Future**

`repository.rs:48-75` — `DISTINCT ON (aggregate_id)` scans all pending events to find the oldest per aggregate. The partial index helps but `DISTINCT ON` still requires sorting all matching rows.

**Current scale:** Fine for a learning project with single-digit services.

**Future fix:** Add `LIMIT` to the inner `oldest_per_aggregate` CTE, or switch to cursor-based processing.

### 6. `delete_published` doesn't filter by status

**Severity: Low** | **Effort: Trivial**

`repository.rs:104-114`:
```sql
DELETE FROM outbox_events WHERE id = $1
```

Can delete any event regardless of status. The status transition trigger only fires on `UPDATE OF status`, not `DELETE`.

**Fix:** Add `AND status IN ('published', 'pending')` or a `BEFORE DELETE` trigger.

### 7. Bulk cleanup without batching

**Severity: Low** | **Effort: Small**

`repository.rs:183-199` — `cleanup_published` deletes all matching events in a single statement. With 100K+ rows, this is a long transaction with WAL pressure. Same issue for `cleanup_processed_events` in `processed.rs:48-61`.

**Fix:** Add `LIMIT 1000` and loop until no rows deleted.

### 8. Trace context is local, not W3C

**Severity: Low** | **Status: Known TODO (bd-8fc)**

`types.rs:137-150` captures `span_name`, `span_target`, `span_id`, `span_module_path` — tracing-crate internals, not W3C traceparent/tracestate. Downstream consumers can't use these to join traces. The `metadata` column stores this but the relay never propagates it to Kafka headers.

### 9. No relay liveness signal

**Severity: Low** | **Effort: Small**

`metrics.rs` provides outbox table metrics, but there's no way to detect if the relay itself is stuck or crashed — the metrics query works even when no relay is running.

**Fix:** Add a `last_relay_heartbeat` timestamp (separate table or Prometheus gauge) so monitoring can alert when the relay hasn't processed events for N minutes.

### 10. Missing backoff timing unit test

**Severity: Low** | **Effort: Small**

The `relay_retries_on_publish_failure` test manually resets `next_retry_at` to test that retry eventually succeeds, but no test verifies the exponential backoff formula `POW(2, LEAST(retry_count + 1, 10))` produces correct delays (2s, 4s, 8s, ... capped at 1024s).

### 11. Publish-then-mark is at-least-once (document, not fix)

**Severity: Awareness**

`relay.rs:153-159` — Publish to Kafka then mark in Postgres are two operations. If `mark_published` fails, the event stays pending and will be re-published → duplicate delivery. This is inherent to the outbox pattern, not a bug. All consumers MUST be idempotent.

**Action:** Document this guarantee explicitly so downstream services know.

### 12. Nits

- `outbox_lag()` and `oldest_unpublished_age_secs()` in `repository.rs` duplicate what `collect_outbox_metrics()` already provides in a single query. Consider removing or marking as convenience aliases.
- `OutboxInsert::from_envelope` uses `.expect("EventEnvelope is always serializable")` — correct (serde_json::Value round-trips are infallible) but inconsistent with error handling philosophy elsewhere.
- The `processed_events` migration template has no comment about scheduling cleanup — operators might not know to run `cleanup_processed_events`.

## Priority Order

| Priority | Issue | Action |
|----------|-------|--------|
| P1 | #1 Batch abort locks events | Fix: continue loop or release locks |
| P1 | #2 Escalation before transition | Fix: swap two lines |
| P2 | #3 Payload size guard | Add validation at insert |
| P2 | #4 Producer timeout | Change to 30000ms |
| P3 | #5 CTE scaling | Future: add LIMIT |
| P3 | #6 delete_published filter | Add status WHERE clause |
| P3 | #7 Batched cleanup | Add LIMIT loop |
| P3 | #9 Relay heartbeat | Add liveness signal |
| P3 | #10 Backoff test | Add unit test |
| P4 | #8 W3C trace context | Tracked as bd-8fc |
| P4 | #11 At-least-once docs | Document guarantee |
| P4 | #12 Nits | Cleanup pass |
