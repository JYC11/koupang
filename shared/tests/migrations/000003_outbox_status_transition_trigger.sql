-- Enforce valid outbox state machine transitions at the DB level.
--
-- Valid transitions:
--   pending   → pending     (retry with backoff, lock/unlock cycles)
--   pending   → published   (successful Kafka publish)
--   pending   → failed      (retries exhausted)
--   published → published   (idempotent mark_published)
--
-- Invalid (rejected with exception):
--   published → pending     (cannot un-publish)
--   published → failed      (published is terminal-success)
--   failed    → pending     (cannot resurrect without manual intervention)
--   failed    → published   (cannot publish a failed event)
--   failed    → failed      (already terminal)

CREATE OR REPLACE FUNCTION enforce_outbox_status_transition() RETURNS trigger AS $$
BEGIN
    -- Self-transitions are always allowed (pending→pending, published→published)
    IF OLD.status = NEW.status THEN
        RETURN NEW;
    END IF;

    -- Valid forward transitions
    IF OLD.status = 'pending' AND NEW.status IN ('published', 'failed') THEN
        RETURN NEW;
    END IF;

    -- Everything else is invalid
    RAISE EXCEPTION 'invalid outbox status transition: % → %', OLD.status, NEW.status
        USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER outbox_enforce_status_transition
    BEFORE UPDATE OF status ON outbox_events
    FOR EACH ROW EXECUTE FUNCTION enforce_outbox_status_transition();
