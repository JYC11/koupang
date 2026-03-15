-- Payment service: double-entry ledger + outbox infrastructure

-- ── Accounts ─────────────────────────────────────────────────

CREATE TABLE accounts (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    account_type    VARCHAR(50) NOT NULL,
    normal_balance  VARCHAR(10) NOT NULL,
    reference_id    UUID NOT NULL,
    currency        VARCHAR(3) NOT NULL DEFAULT 'USD',
    CONSTRAINT chk_normal_balance CHECK (normal_balance IN ('debit', 'credit')),
    CONSTRAINT chk_account_type CHECK (account_type IN (
        'buyer', 'gateway_holding', 'platform_revenue', 'seller_payable'
    )),
    CONSTRAINT uq_accounts UNIQUE (reference_id, account_type, currency)
);

CREATE INDEX idx_accounts_ref ON accounts (reference_id, account_type);

-- ── Ledger Transactions ──────────────────────────────────────

CREATE TABLE ledger_transactions (
    id                UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    order_id          UUID NOT NULL,
    transaction_type  VARCHAR(50) NOT NULL,
    status            VARCHAR(20) NOT NULL DEFAULT 'pending',
    idempotency_key   VARCHAR(255) NOT NULL UNIQUE,
    gateway_reference VARCHAR(255),
    metadata          JSONB NOT NULL DEFAULT '{}',
    CONSTRAINT chk_transaction_type CHECK (transaction_type IN (
        'authorization', 'capture', 'void', 'refund'
    )),
    CONSTRAINT chk_status CHECK (status IN ('pending', 'posted', 'discarded'))
);

CREATE INDEX idx_ledger_tx_order ON ledger_transactions (order_id, created_at DESC);

-- ── Ledger Entries ───────────────────────────────────────────

CREATE TABLE ledger_entries (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    transaction_id  UUID NOT NULL REFERENCES ledger_transactions (id),
    account_id      UUID NOT NULL REFERENCES accounts (id),
    direction       VARCHAR(10) NOT NULL,
    amount          NUMERIC(19, 4) NOT NULL,
    CONSTRAINT chk_amount CHECK (amount > 0),
    CONSTRAINT chk_direction CHECK (direction IN ('debit', 'credit'))
);

CREATE INDEX idx_entries_tx ON ledger_entries (transaction_id);
CREATE INDEX idx_entries_account ON ledger_entries (account_id);

-- ── Account Balances View ────────────────────────────────────

CREATE VIEW account_balances AS
SELECT
    a.id AS account_id,
    a.account_type,
    a.reference_id,
    a.normal_balance,
    a.currency,
    COALESCE(SUM(CASE WHEN e.direction = 'debit' AND t.id IS NOT NULL THEN e.amount ELSE 0 END), 0) AS total_debits,
    COALESCE(SUM(CASE WHEN e.direction = 'credit' AND t.id IS NOT NULL THEN e.amount ELSE 0 END), 0) AS total_credits,
    CASE a.normal_balance
        WHEN 'debit' THEN COALESCE(SUM(CASE WHEN t.id IS NOT NULL THEN (CASE WHEN e.direction = 'debit' THEN e.amount ELSE -e.amount END) ELSE 0 END), 0)
        WHEN 'credit' THEN COALESCE(SUM(CASE WHEN t.id IS NOT NULL THEN (CASE WHEN e.direction = 'credit' THEN e.amount ELSE -e.amount END) ELSE 0 END), 0)
    END AS balance
FROM accounts a
LEFT JOIN ledger_entries e ON e.account_id = a.id
LEFT JOIN ledger_transactions t ON t.id = e.transaction_id AND t.status = 'posted'
GROUP BY a.id, a.account_type, a.reference_id, a.normal_balance, a.currency;

-- ── Outbox (producer) ────────────────────────────────────────

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

-- ── Processed events (consumer idempotency) ──────────────────

CREATE TABLE processed_events (
    event_id        UUID NOT NULL,
    event_type      VARCHAR(100) NOT NULL,
    source_service  VARCHAR(100) NOT NULL,
    consumer_group  VARCHAR(100) NOT NULL,
    processed_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (event_id, consumer_group)
);

CREATE INDEX idx_processed_events_at ON processed_events (processed_at);
