# Outbox Lifecycle Reference

## Happy Path

```
Service Transaction                    Relay (background loop)              Kafka
─────────────────                      ───────────────────────              ─────
1. BEGIN tx
2. INSERT order row
3. INSERT outbox_events
   (status=pending,
    locked_by=NULL,
    retry_count=0)
4. COMMIT tx
   ├─ trigger fires ──────────────►   pg_notify('outbox_events', id)
                                       ↓ PgListener wakes up
                                      5. claim_batch(pool, 50, "relay-1")
                                         CTE:
                                         ├─ DISTINCT ON(aggregate_id) oldest pending
                                         ├─ WHERE next_retry_at <= NOW()
                                         ├─ WHERE locked_by IS NULL
                                         └─ FOR UPDATE SKIP LOCKED
                                         → sets locked_by="relay-1", locked_at=NOW()

                                      6. Publish to Kafka ──────────────►  topic: order.events
                                                                           partition_key: aggregate_id
                                      7. mark_published(pool, id)
                                         → status='published'
                                         → published_at=NOW()
                                         → locked_by=NULL

                                         OR (delete_on_publish mode):
                                      7. delete_published(pool, id)
                                         → row removed entirely
```

Key guarantees:
- Event and business data committed atomically (same transaction)
- LISTEN/NOTIFY wakes the relay within milliseconds of commit
- Event delivered to Kafka exactly once

## Unhappy Path 1: Kafka Publish Fails (Transient)

```
Relay                                              outbox_events row
─────                                              ──────────────────
1. claim_batch → locked_by="relay-1"               status=pending, retry_count=0

2. Publish to Kafka → TIMEOUT / connection refused

3. mark_retry_or_failed(id, "kafka timeout")
   ├─ retry_count: 0 → 1                          retry_count=1
   ├─ status stays 'pending'                       status=pending
   ├─ next_retry_at = NOW() + 2^1 = 2s            next_retry_at=+2s
   ├─ last_error = "kafka timeout"
   └─ locked_by = NULL                             (unlocked, but not yet retryable)

   ... 2 seconds pass ...

4. claim_batch → picks it up again                 locked_by="relay-1"

5. Publish to Kafka → TIMEOUT again

6. mark_retry_or_failed(id, "kafka timeout")
   ├─ retry_count: 1 → 2                          retry_count=2
   ├─ next_retry_at = NOW() + 2^2 = 4s            next_retry_at=+4s
   └─ locked_by = NULL

   ... 4 seconds pass ...

7. claim_batch → picks it up again
8. Publish to Kafka → SUCCESS
9. mark_published(id)                              status=published
```

### Backoff schedule (capped at 2^10)

| Retry # | Delay    | Cumulative |
|---------|----------|------------|
| 1       | 2s       | 2s         |
| 2       | 4s       | 6s         |
| 3       | 8s       | 14s        |
| 4       | 16s      | 30s        |
| 5       | 32s      | ~1min      |
| 6       | 64s      | ~2min      |
| 7       | 128s     | ~4min      |
| 8       | 256s     | ~8min      |
| 9       | 512s     | ~17min     |
| 10+     | 1024s    | capped     |

## Unhappy Path 2: Retries Exhausted (Permanent Failure)

```
Relay                                              outbox_events row
─────                                              ──────────────────
(after 10 failed retries with max_retries=10)

1. claim_batch → locked_by="relay-1"               retry_count=9

2. Publish to Kafka → FAILS

3. mark_retry_or_failed(id, "topic deleted")
   ├─ retry_count + 1 (10) >= max_retries (10)
   ├─ status → 'failed'                           status=FAILED (terminal)
   ├─ last_error = "topic deleted"
   └─ locked_by = NULL

4. FailureEscalation::on_permanent_failure(event)
   └─ Default (LogFailureEscalation): logs structured error
   └─ Custom: push to DLQ topic, alert PagerDuty, etc.

   Event is now TERMINAL — never claimed again.
```

Edge case: `max_retries=0` → first failure immediately transitions to `failed`.

## Unhappy Path 3: Relay Crashes (Stale Lock)

```
Relay-1                          Maintenance loop              Relay-2
───────                          ────────────────              ───────
1. claim_batch
   locked_by="relay-1"
   locked_at=10:00:00

2. CRASH (OOM, network, panic)

                                 3. release_stale_locks(60s)
                                    at 10:01:05:
                                    locked_at (10:00:00) < NOW() - 60s
                                    → locked_by=NULL
                                    → locked_at=NULL
                                    (retry_count unchanged)

                                                               4. claim_batch
                                                                  → picks up the event
                                                                  locked_by="relay-2"
                                                               5. Publish → SUCCESS
                                                               6. mark_published

```

Safety: `release_stale_locks` only releases locks where `locked_at < NOW() - timeout`.
Fresh locks (< 60s old) are never released.

## Unhappy Path 4: Service Transaction Rolls Back

```
Service
───────
1. BEGIN tx
2. INSERT order row
3. INSERT outbox_events
4. ROLLBACK tx (validation fails, constraint violation, etc.)

→ Both the order AND the outbox event are discarded atomically.
→ No phantom event. No orphan message. This is the core guarantee.
```

## Per-Aggregate Ordering

Events for the same aggregate are always delivered in insertion order.

```
outbox_events table:
┌────────────────┬──────────┬───────────┬────────┐
│ id (created_at)│ agg_id   │ event_type│ status │
├────────────────┼──────────┼───────────┼────────┤
│ 1 (10:00:00)   │ order-A  │ Created   │ pending│  ← claim_batch returns THIS
│ 2 (10:00:01)   │ order-A  │ Paid      │ pending│  ← blocked (same aggregate)
│ 3 (10:00:02)   │ order-A  │ Shipped   │ pending│  ← blocked (same aggregate)
│ 4 (10:00:00)   │ order-B  │ Created   │ pending│  ← also returned (different agg)
└────────────────┴──────────┴───────────┴────────┘

claim_batch(10, "relay-1") → returns events 1 and 4 only.

After mark_published(event 1):
  → event 2 becomes the oldest pending for order-A
  → next claim_batch returns event 2

After mark_published(event 2):
  → event 3 becomes claimable
```

What if event 1 fails permanently?

```
Event 1 → status='failed' (terminal)
Event 2 → NOW the oldest *pending* event for order-A
         → claim_batch returns it
         → ordering is maintained: failed events are skipped, not blocking
```

## Concurrent Relay Safety

```
Relay-1                                Relay-2
───────                                ───────
claim_batch(10, "relay-1")             claim_batch(10, "relay-2")
        │                                      │
        └──── both execute CTE simultaneously ─┘
              FOR UPDATE SKIP LOCKED

              Postgres guarantees:
              • One relay wins the row lock
              • Other relay SKIPs that row
              • Zero duplicate claims

Result: 10 events → relay-1 gets some, relay-2 gets the rest, no overlap.
```

## Consumer Side (Downstream Service)

```
Consumer (e.g. Catalog listening to order.events)
─────────────────────────────────────────────────
1. Receive Kafka message

2. is_event_processed(pool, event_id)?
   ├─ true  → SKIP (already handled, Kafka redelivery)
   └─ false → continue

3. BEGIN tx
4. Handle event (e.g. reserve inventory)
5. mark_event_processed(pool, event_id, "OrderCreated", "order")
6. COMMIT tx

7. ACK Kafka offset
```

Idempotency: `mark_event_processed` uses `ON CONFLICT DO NOTHING` — calling it twice is safe.

## Maintenance Loops

```
┌─────────────────────────────────────────────────────┐
│ Relay background tasks (configurable via RelayConfig)│
├─────────────────────────────────────────────────────┤
│ 1. Main loop: PgListener + poll_interval (500ms)    │
│    claim_batch → publish → mark_published           │
│                                                     │
│ 2. Stale lock cleanup (every stale_lock_timeout=60s)│
│    release_stale_locks(60s) → unlock crashed relays │
│                                                     │
│ 3. Published event cleanup (every cleanup_interval) │
│    cleanup_published(7 days) → delete old rows      │
│                                                     │
│ 4. Consumer cleanup (periodic)                      │
│    cleanup_processed_events(30 days)                │
└─────────────────────────────────────────────────────┘
```

## State Machine Summary

Enforced at the DB level by the `outbox_enforce_status_transition` trigger
(migration `000003`). Invalid transitions raise a `check_violation` exception.

```
                 insert
                   │
                   ▼
              ┌─────────┐
              │ PENDING  │◄──────── release_stale_locks()
              └────┬─────┘              ▲
                   │                    │
              claim_batch          mark_retry_or_failed
              (lock acquired)      (retry_count < max)
                   │                    │
                   ▼                    │
              ┌─────────┐              │
              │ LOCKED   │──────────────┘
              │(pending) │
              └────┬─────┘
                   │
          ┌────────┼────────┐
          │        │        │
     publish ok  publish  retry_count
          │      fails    >= max_retries
          ▼        │        │
   ┌──────────┐    │        ▼
   │PUBLISHED │    │   ┌─────────┐
   └──────┬───┘    │   │ FAILED  │ (terminal)
          │        │   └─────────┘
    cleanup_published    FailureEscalation
    (after 7 days)       ::on_permanent_failure()
          │
          ▼
       DELETED
```
