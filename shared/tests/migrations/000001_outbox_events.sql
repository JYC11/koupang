CREATE TABLE outbox_events (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    aggregate_type  VARCHAR(100) NOT NULL,
    aggregate_id    UUID NOT NULL,
    event_type      VARCHAR(100) NOT NULL,
    event_id        UUID NOT NULL UNIQUE,
    topic           VARCHAR(255) NOT NULL,
    partition_key   VARCHAR(255) NOT NULL,
    payload         JSONB NOT NULL,
    metadata        JSONB,
    status          VARCHAR(20) NOT NULL DEFAULT 'pending',
    published_at    TIMESTAMPTZ,
    locked_by       VARCHAR(255),
    locked_at       TIMESTAMPTZ,
    retry_count     INTEGER NOT NULL DEFAULT 0,
    max_retries     INTEGER NOT NULL DEFAULT 10,
    last_error      TEXT,
    next_retry_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_outbox_status CHECK (status IN ('pending', 'published', 'failed'))
);

-- Hot path: relay claims pending events (oldest per aggregate, respects next_retry_at)
CREATE INDEX idx_outbox_pending ON outbox_events (next_retry_at, aggregate_id, created_at)
    WHERE status = 'pending';

-- Cleanup: find old published events
CREATE INDEX idx_outbox_published ON outbox_events (published_at)
    WHERE status = 'published';

-- Stale lock detection
CREATE INDEX idx_outbox_locked ON outbox_events (locked_at)
    WHERE locked_by IS NOT NULL AND status = 'pending';

-- LISTEN/NOTIFY trigger for near-real-time relay wakeup
CREATE OR REPLACE FUNCTION notify_outbox_insert() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('outbox_events', NEW.id::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER outbox_events_after_insert
    AFTER INSERT ON outbox_events
    FOR EACH ROW EXECUTE FUNCTION notify_outbox_insert();
