ALTER TABLE users ADD COLUMN email_verified BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE email_verification_tokens (
    id          UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token       VARCHAR(64) NOT NULL UNIQUE,
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ
);

CREATE INDEX idx_evt_token ON email_verification_tokens(token);
CREATE INDEX idx_evt_user_id ON email_verification_tokens(user_id);
