-- Inventory reservation system for order/payment saga
-- Adds reserved_quantity tracking, reservation records, and outbox infrastructure

-- ── SKU inventory tracking ──────────────────────────────────

ALTER TABLE skus ADD COLUMN reserved_quantity INTEGER NOT NULL DEFAULT 0;
ALTER TABLE skus ADD CONSTRAINT chk_reserved_quantity CHECK (reserved_quantity >= 0);

-- ── Inventory reservations ──────────────────────────────────

CREATE TABLE inventory_reservations (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    order_id        UUID NOT NULL,
    sku_id          UUID NOT NULL REFERENCES skus (id),
    quantity        INTEGER NOT NULL,
    status          VARCHAR(20) NOT NULL DEFAULT 'reserved',
    released_at     TIMESTAMPTZ,
    confirmed_at    TIMESTAMPTZ,
    CONSTRAINT chk_reservation_quantity CHECK (quantity > 0),
    CONSTRAINT chk_reservation_status CHECK (status IN ('reserved', 'released', 'confirmed')),
    CONSTRAINT uq_reservation_order_sku UNIQUE (order_id, sku_id)
);

CREATE INDEX idx_reservations_order ON inventory_reservations (order_id);
CREATE INDEX idx_reservations_sku ON inventory_reservations (sku_id) WHERE status = 'reserved';

-- ── SKU availability view ───────────────────────────────────
-- available = stock_quantity - reserved_quantity

CREATE VIEW sku_availability AS
SELECT
    s.id AS sku_id,
    s.product_id,
    s.sku_code,
    s.stock_quantity,
    s.reserved_quantity,
    (s.stock_quantity - s.reserved_quantity) AS available_quantity
FROM skus s
WHERE s.deleted_at IS NULL;

-- ── Outbox (producer) ───────────────────────────────────────

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

CREATE INDEX idx_outbox_pending ON outbox_events (next_retry_at, aggregate_id, created_at)
    WHERE status = 'pending';
CREATE INDEX idx_outbox_published ON outbox_events (published_at)
    WHERE status = 'published';
CREATE INDEX idx_outbox_locked ON outbox_events (locked_at)
    WHERE locked_by IS NOT NULL AND status = 'pending';

CREATE OR REPLACE FUNCTION notify_outbox_insert() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('outbox_events', NEW.id::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER outbox_events_after_insert
    AFTER INSERT ON outbox_events
    FOR EACH ROW EXECUTE FUNCTION notify_outbox_insert();

CREATE OR REPLACE FUNCTION enforce_outbox_status_transition() RETURNS trigger AS $$
BEGIN
    IF OLD.status = NEW.status THEN
        RETURN NEW;
    END IF;
    IF OLD.status = 'pending' AND NEW.status IN ('published', 'failed') THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'invalid outbox status transition: % → %', OLD.status, NEW.status
        USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER outbox_enforce_status_transition
    BEFORE UPDATE OF status ON outbox_events
    FOR EACH ROW EXECUTE FUNCTION enforce_outbox_status_transition();

-- ── Processed events (consumer idempotency) ─────────────────

CREATE TABLE processed_events (
    event_id        UUID NOT NULL,
    event_type      VARCHAR(100) NOT NULL,
    source_service  VARCHAR(100) NOT NULL,
    consumer_group  VARCHAR(100) NOT NULL,
    processed_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (event_id, consumer_group)
);

CREATE INDEX idx_processed_events_at ON processed_events (processed_at);
