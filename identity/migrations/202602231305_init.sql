CREATE TABLE users (
    id          UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ,
    deleted_at  TIMESTAMPTZ,
    username    VARCHAR(255) NOT NULL UNIQUE,
    password    TEXT NOT NULL,
    email       VARCHAR(255) NOT NULL UNIQUE,
    phone       VARCHAR(50) NOT NULL,
    role        VARCHAR(50) NOT NULL
);