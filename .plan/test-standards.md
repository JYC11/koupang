# Test Standards

Standards for what each test layer covers, what it should NOT duplicate, and how to decide where a new test belongs.

---

## Principles

1. **Each test should justify its infrastructure cost.** A test that needs a database should assert something that only a database can verify.
2. **No duplicate assertions across layers.** If a behavior is tested at the router layer, don't re-test the same happy path at the service and repository layers.
3. **Test at the highest layer that naturally exercises the behavior.** Happy-path CRUD flows through all layers, so test it once at the router level.
4. **Test at the lowest layer that isolates the concern.** SQL constraints, JOIN behavior, and ltree paths are repository concerns — test them there.

---

## Layer Responsibilities

### Value Objects — Pure Unit Tests

**Infrastructure:** None (`#[test]`, synchronous, no I/O)

**What to test:**
- Validation rules (min/max, format, required characters)
- Normalization (lowercasing, trimming, slug generation)
- Rejection messages for invalid input
- Boundary values

**Examples:** `Quantity(0)` rejected, `Email` lowercased, `Password` requires special char

---

### Repository Tests — SQL Correctness

**Infrastructure:** Shared TestDb (one container per binary)

**What to test:**
- Constraint behavior: unique violations, CHECK constraints, FK violations
- Negative/edge cases at the SQL level: negative stock rejected, duplicate slug
- JOIN behavior: joined fields present when FK set, NULL when FK absent
- Internal helper functions: `has_children()`, `brand_exists()`, `is_brand_in_category()`
- Postgres-specific features: ltree path correctness, `<@`/`@>` queries
- Query correctness for complex operations: pagination SQL, filter SQL

**What NOT to test here:**
- CRUD happy paths (create → get → verify fields). These are covered by router tests.
- List operations that just return rows without complex logic
- Simple delete operations

---

### Service Tests — Business Logic & Auth

**Infrastructure:** Shared TestDb (one container per binary)

**What to test:**
- Auth guard enforcement: `require_admin()` rejects non-admin, `require_access()` rejects non-owner
- Ownership checks: seller A cannot modify seller B's product
- Business guards: "cannot delete brand with associated products", "cannot delete category with children"
- FK validation: nonexistent category/brand rejected, brand-not-in-category rejected
- Cache behavior: cache hit/miss, cache eviction on write (identity Redis tests)
- Complex workflows that add logic beyond the repository: password hashing, token generation, email verification flow

**What NOT to test here:**
- CRUD happy paths that the service just delegates to the repository
- HTTP status codes (that's a router concern)
- Request deserialization (that's a router concern)

---

### Router Tests — HTTP Contract

**Infrastructure:** Shared TestDb (one container per binary), full axum router via `tower::oneshot`

**What to test:**
- **Happy-path CRUD** — the canonical integration test for create/read/update/delete flows
- HTTP status codes: 200, 201, 400, 401, 403, 404, 422
- Auth middleware: unauthenticated → 401, wrong role → 403
- Request deserialization: malformed body → 422
- Response shape: correct JSON structure, computed fields present
- Pagination: cursor, limit, next/prev cursor
- Filters: query parameter wiring (category, brand, price range, search)
- Multi-step flows: create → update → get → delete → verify 404

**What NOT to test here:**
- SQL constraint details (tested at repository)
- Internal business rule error messages (tested at service)
- Value object validation rules (tested as unit tests)

---

### gRPC Tests — Protocol Contract

**Infrastructure:** Shared TestDb + in-process tonic server

**What to test:**
- Field mapping: all fields correctly serialized to protobuf
- gRPC error codes: `NotFound`, `InvalidArgument`
- Edge cases: empty ID, invalid UUID format, deleted entity

---

## Decision Flowchart

When adding a new test, ask:

```
Is this about input validation rules?
  → Value object unit test

Is this about SQL behavior (constraints, JOINs, complex queries)?
  → Repository test

Is this about auth, ownership, or business rule enforcement?
  → Service test

Is this about HTTP status codes, response shape, or request parsing?
  → Router test

Is this a happy-path CRUD flow?
  → Router test (one canonical test covers all layers)
```

---

## Shared Container Infrastructure

Tests use a shared Postgres container per test binary (not per test). Each test gets its own database created from a pre-migrated template via `CREATE DATABASE ... TEMPLATE`, which is a file-level copy (~50-100ms vs ~3-8s for a new container).

```rust
// This is all you need — the shared container is managed internally
let db = TestDb::start("./migrations").await;
let pool = db.pool.clone();
```

Redis tests share a single container, flushed between tests via `FLUSHDB`.

---

## Applying to New Services

When writing tests for a new service:

1. Start with value object unit tests (fast feedback, no infra)
2. Write router tests for all endpoints (canonical happy-path + HTTP contract)
3. Add service tests only for auth guards and business rules
4. Add repository tests only for SQL-specific concerns
5. Verify no test duplicates an assertion already covered at another layer
