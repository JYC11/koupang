---
name: test-guide
description: >
  Guide for writing tests following the project's layered test standards. Use when the user says
  "write tests", "add tests", "test strategy", "what tests to write", "test this module",
  "where should this test go", or is deciding which test layer to use.
---

# Test Standards

Each test justifies its infrastructure cost. No duplicate assertions across layers.

## Decision Flowchart

```
Input validation rules?           → Value object unit test
SQL behavior (constraints, JOINs)? → Repository test
Auth, ownership, business rules?   → Service test
HTTP status, response shape?       → Router test
Happy-path CRUD flow?              → Router test (covers all layers)
```

## Layer Guide

### Value Objects — Pure Unit Tests
**Infra:** None (`#[test]`, sync, no I/O)
**Test:** validation rules, normalization, rejection messages, boundary values

### Repository — SQL Correctness
**Infra:** Shared TestDb
**Test:** constraints (unique, CHECK, FK), JOINs, ltree paths, complex queries, pagination SQL
**Skip:** CRUD happy paths (covered by router tests)

### Service — Business Logic & Auth
**Infra:** Shared TestDb
**Test:** auth guards (`require_admin`, `require_access`), ownership checks, FK validation, cache behavior, complex workflows (password hashing, token generation)
**Skip:** CRUD happy paths, HTTP status codes, request deserialization

### Router — HTTP Contract
**Infra:** Shared TestDb + full axum router via `tower::oneshot`
**Test:** happy-path CRUD, HTTP status codes, auth middleware (401/403), request deserialization (422), response shape, pagination, filters, multi-step flows
**Skip:** SQL constraint details, internal error messages, VO validation rules

## Test Infrastructure

```rust
// tests/common/mod.rs — per-service helpers
pub async fn test_db() -> TestDb {
    TestDb::start("./migrations").await
}
pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_jwt(pool, test_auth_config())
}
```

Container sharing and test utilities (TestDb, TestRedis, TestKafka, TestConsumer) are documented in [shared/CLAUDE.md](../../shared/CLAUDE.md#test-utilities-feature-test-utils).

### Event Integration — Outbox + Kafka
**Infra:** Shared TestDb + TestKafka
**Test:** full relay cycle (insert → claim → publish → consume), payload fidelity through Kafka, per-aggregate ordering, consumer idempotency, delete-on-publish mode, retry-then-publish
**See:** [shared/CLAUDE.md](../../shared/CLAUDE.md#transactional-outbox-events--outbox-modules) for outbox + Kafka API examples

## Order for New Services

1. Value object unit tests (fast feedback, no infra)
2. Router tests for all endpoints (canonical happy-path + HTTP contract)
3. Service tests only for auth guards and business rules
4. Repository tests only for SQL-specific concerns
5. Event integration tests for services that publish domain events
6. Verify no test duplicates an assertion at another layer
