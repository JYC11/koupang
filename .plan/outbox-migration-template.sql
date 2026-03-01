-- ============================================================================
-- Outbox Migration Template
-- ============================================================================
-- Copy this into your service's migrations/ directory when adopting the
-- transactional outbox pattern. Requires the uuidv7() extension (already
-- enabled in the shared TestDb and production init_db()).
--
-- Two tables:
--   1. outbox_events    — producer side (write events in same transaction)
--   2. processed_events — consumer side (idempotency deduplication)
--
-- Usage:
--   make migration SERVICE=order NAME=add_outbox
--   # Paste the contents below into the generated .sql file
-- ============================================================================

-- ── Producer: outbox_events ─────────────────────────────────────────────────

CREATE TABLE outbox_events (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Aggregate identity (for per-aggregate ordering)
    aggregate_type  VARCHAR(100) NOT NULL,   -- e.g. 'Order', 'Payment'
    aggregate_id    UUID NOT NULL,

    -- Event identity
    event_type      VARCHAR(100) NOT NULL,   -- e.g. 'OrderCreated'
    event_id        UUID NOT NULL UNIQUE,    -- dedup key (from EventMetadata)

    -- Kafka routing
    topic           VARCHAR(255) NOT NULL,   -- target Kafka topic
    partition_key   VARCHAR(255) NOT NULL,   -- Kafka partition key (= aggregate_id)

    -- Payload
    payload         JSONB NOT NULL,          -- serialized EventEnvelope
    metadata        JSONB,                   -- trace context (W3C traceparent)

    -- Relay lifecycle
    status          VARCHAR(20) NOT NULL DEFAULT 'pending',
    published_at    TIMESTAMPTZ,
    locked_by       VARCHAR(255),            -- relay instance_id
    locked_at       TIMESTAMPTZ,

    -- Retry with exponential backoff
    retry_count     INTEGER NOT NULL DEFAULT 0,
    max_retries     INTEGER NOT NULL DEFAULT 10,
    last_error      TEXT,
    next_retry_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT chk_outbox_status CHECK (status IN ('pending', 'published', 'failed'))
);

-- Hot path: relay claims oldest pending event per aggregate
CREATE INDEX idx_outbox_pending ON outbox_events (aggregate_id, created_at)
    WHERE status = 'pending';

-- Cleanup: find old published events for deletion
CREATE INDEX idx_outbox_published ON outbox_events (published_at)
    WHERE status = 'published';

-- Stale lock detection: find locked events whose relay may have crashed
CREATE INDEX idx_outbox_locked ON outbox_events (locked_at)
    WHERE locked_by IS NOT NULL AND status = 'pending';

-- LISTEN/NOTIFY trigger for near-real-time relay wakeup
-- (relay uses PgListener on 'outbox_events' channel, falls back to polling)
CREATE OR REPLACE FUNCTION notify_outbox_insert() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('outbox_events', NEW.id::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER outbox_events_after_insert
    AFTER INSERT ON outbox_events
    FOR EACH ROW EXECUTE FUNCTION notify_outbox_insert();

-- ── Consumer: processed_events ──────────────────────────────────────────────
-- Tracks which events this service has already processed (idempotency).
-- Consumers call is_event_processed() before handling, then mark_event_processed()
-- after successful processing. Old records are cleaned up periodically.

CREATE TABLE processed_events (
    event_id        UUID PRIMARY KEY,
    event_type      VARCHAR(100) NOT NULL,
    source_service  VARCHAR(100) NOT NULL,
    processed_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_processed_events_at ON processed_events (processed_at);
