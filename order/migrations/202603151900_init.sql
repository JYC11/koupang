-- Order service: core tables + outbox infrastructure

CREATE TABLE orders (
    id                UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ,
    buyer_id          UUID NOT NULL,
    status            VARCHAR(50) NOT NULL DEFAULT 'pending',
    total_amount      NUMERIC(19, 4) NOT NULL,
    currency          VARCHAR(3) NOT NULL DEFAULT 'USD',
    idempotency_key   VARCHAR(255) NOT NULL UNIQUE,
    shipping_address  JSONB NOT NULL DEFAULT '{}',
    cancelled_reason  TEXT,
    CONSTRAINT chk_orders_total CHECK (total_amount >= 0),
    CONSTRAINT chk_orders_status CHECK (status IN (
        'pending', 'inventory_reserved', 'payment_authorized',
        'confirmed', 'shipped', 'delivered', 'cancelled', 'returned'
    ))
);

CREATE INDEX idx_orders_buyer ON orders (buyer_id, created_at DESC);
CREATE INDEX idx_orders_status ON orders (status) WHERE status NOT IN ('delivered', 'cancelled', 'returned');

CREATE TABLE order_items (
    id            UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    order_id      UUID NOT NULL REFERENCES orders (id),
    product_id    UUID NOT NULL,
    sku_id        UUID NOT NULL,
    product_name  VARCHAR(500) NOT NULL,
    sku_code      VARCHAR(100) NOT NULL,
    quantity      INTEGER NOT NULL,
    seller_id     UUID NOT NULL,
    unit_price    NUMERIC(19, 4) NOT NULL,
    total_price   NUMERIC(19, 4) NOT NULL,
    CONSTRAINT chk_quantity CHECK (quantity > 0),
    CONSTRAINT chk_unit_price CHECK (unit_price >= 0),
    CONSTRAINT chk_total_price CHECK (total_price >= 0)
);

CREATE INDEX idx_order_items_order ON order_items (order_id);
CREATE INDEX idx_order_items_seller ON order_items (seller_id);

-- ── Outbox (producer) ────────────────────────────────────────────────────

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

-- ── Processed events (consumer idempotency) ──────────────────────────────

CREATE TABLE processed_events (
    event_id        UUID NOT NULL,
    event_type      VARCHAR(100) NOT NULL,
    source_service  VARCHAR(100) NOT NULL,
    consumer_group  VARCHAR(100) NOT NULL,
    processed_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (event_id, consumer_group)
);

CREATE INDEX idx_processed_events_at ON processed_events (processed_at);
