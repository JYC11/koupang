-- Persistent job system table, indexes, and triggers.
-- Supports one-shot, delayed, and recurring jobs with at-least-once execution.

CREATE TABLE persistent_jobs (
    id                UUID PRIMARY KEY DEFAULT uuidv7(),
    job_type          VARCHAR(255) NOT NULL,
    payload           JSONB NOT NULL DEFAULT '{}',
    status            VARCHAR(20) NOT NULL DEFAULT 'pending',
    schedule          VARCHAR(255),
    dedup_key         VARCHAR(255),
    max_retries       INTEGER NOT NULL DEFAULT 5,
    timeout_seconds   INTEGER NOT NULL DEFAULT 300,
    attempts          INTEGER NOT NULL DEFAULT 0,
    last_error        TEXT,
    locked_by         VARCHAR(255),
    locked_at         TIMESTAMPTZ,
    next_run_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_job_status CHECK (
        status IN ('pending', 'running', 'completed', 'failed', 'dead_lettered', 'cancelled')
    )
);

-- ── Partial indexes (D2) ───────────────────────────────────────────

-- Claim query: pending jobs ready to run
CREATE INDEX idx_jobs_pending ON persistent_jobs (next_run_at)
    WHERE status = 'pending';

-- Dedup: unique constraint for Skip strategy
CREATE UNIQUE INDEX idx_jobs_dedup ON persistent_jobs (job_type, dedup_key)
    WHERE status IN ('pending', 'running') AND dedup_key IS NOT NULL;

-- Stale lock detection
CREATE INDEX idx_jobs_stale_locks ON persistent_jobs (locked_at)
    WHERE status = 'running';

-- Cleanup: find old completed jobs
CREATE INDEX idx_jobs_cleanup ON persistent_jobs (updated_at)
    WHERE status = 'completed';

-- ── Updated_at trigger ─────────────────────────────────────────────

CREATE OR REPLACE FUNCTION persistent_jobs_set_updated_at() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER persistent_jobs_before_update
    BEFORE UPDATE ON persistent_jobs
    FOR EACH ROW EXECUTE FUNCTION persistent_jobs_set_updated_at();

-- ── Status transition trigger (R5) ─────────────────────────────────

CREATE OR REPLACE FUNCTION enforce_job_status_transition() RETURNS trigger AS $$
BEGIN
    -- Self-transitions always allowed
    IF OLD.status = NEW.status THEN RETURN NEW; END IF;

    -- Universal transitions
    IF OLD.status = 'pending'       AND NEW.status = 'running'       THEN RETURN NEW; END IF;
    IF OLD.status = 'running'       AND NEW.status = 'completed'     THEN RETURN NEW; END IF;
    IF OLD.status = 'running'       AND NEW.status = 'failed'        THEN RETURN NEW; END IF;
    IF OLD.status = 'running'       AND NEW.status = 'dead_lettered' THEN RETURN NEW; END IF;
    IF OLD.status = 'running'       AND NEW.status = 'pending'       THEN RETURN NEW; END IF;
    IF OLD.status = 'pending'       AND NEW.status = 'cancelled'     THEN RETURN NEW; END IF;
    IF OLD.status = 'dead_lettered' AND NEW.status = 'pending'       THEN RETURN NEW; END IF;
    IF OLD.status = 'failed'        AND NEW.status = 'pending'       THEN RETURN NEW; END IF;

    -- Recurring-only: completed -> pending (reset-in-place)
    IF OLD.status = 'completed' AND NEW.status = 'pending' THEN
        IF NEW.schedule IS NOT NULL THEN RETURN NEW; END IF;
        RAISE EXCEPTION 'completed -> pending only valid for recurring jobs (schedule IS NOT NULL)'
            USING ERRCODE = 'check_violation';
    END IF;

    RAISE EXCEPTION 'invalid job status transition: % -> %', OLD.status, NEW.status
        USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER job_enforce_status_transition
    BEFORE UPDATE OF status ON persistent_jobs
    FOR EACH ROW EXECUTE FUNCTION enforce_job_status_transition();

-- ── NOTIFY trigger (D16) ───────────────────────────────────────────

CREATE OR REPLACE FUNCTION persistent_jobs_notify() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('persistent_jobs', NEW.id::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER persistent_jobs_after_insert
    AFTER INSERT ON persistent_jobs
    FOR EACH ROW EXECUTE FUNCTION persistent_jobs_notify();
