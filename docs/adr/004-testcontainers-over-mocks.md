# ADR-004: Testcontainers over mocking for database tests

**Date:** 2026-02-23
**Status:** Accepted

## Context

Need a testing strategy for database-dependent code. Options: mock the repository trait, use an in-memory SQLite substitute, or spin up real Postgres per test.

## Decision

Use testcontainers to spin up ephemeral Postgres 18 (and Redis) containers for tests. Tests run single-threaded (`--test-threads=1`) to avoid DB conflicts.

## Consequences

- Tests hit real Postgres — no behavior mismatch between test and production
- Migrations run on the test container, so schema issues are caught immediately
- Slower than mocks (~2-3s container startup) but the shared `TestDb` utility makes it painless
- Single-threaded execution prevents flaky tests but makes the test suite slower at scale
- Docker must be running locally to run tests (make sure docker is installed and running)
