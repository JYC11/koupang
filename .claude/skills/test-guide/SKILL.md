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
// tests/common/mod.rs
pub async fn test_db() -> TestDb {
    TestDb::start("./migrations").await
}
pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_jwt(pool, test_auth_config())
}
pub fn seller_user() -> CurrentUser {
    CurrentUser { id: Uuid::new_v4(), role: Role::Seller }
}

// tests/<module>/<layer>_test.rs
#[tokio::test]
async fn test_create_thing() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let user = seller_user();
    let result = service.create_thing(&user, sample_req()).await;
    assert!(result.is_ok());
}
```

Shared Postgres container per test binary. Each test gets its own DB via `CREATE DATABASE ... TEMPLATE` (~50-100ms). Redis tests share a container, flushed via `FLUSHDB`.

## Order for New Services

1. Value object unit tests (fast feedback, no infra)
2. Router tests for all endpoints (canonical happy-path + HTTP contract)
3. Service tests only for auth guards and business rules
4. Repository tests only for SQL-specific concerns
5. Verify no test duplicates an assertion at another layer
