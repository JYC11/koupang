# Review Standards — koupang

## Architecture

layers: routes → service → domain → repository

components:
  - routes: axum handler functions (HTTP entry point)
  - service: Business logic orchestration, transaction boundaries
  - domain: Core types, value objects, newtypes, state machines
  - repository: sqlx queries, data access

## Test Patterns

test_file_patterns:
  - "**/tests/**"
  - "**/*_test.rs"

test_command: "make test SERVICE=<name>"
test_framework: "Rust (#[tokio::test] with testcontainers, serial execution)"

## Review Checklist — CRITICAL (blocks merge)

### SQL & Data Safety
- String interpolation/format! in SQL — use sqlx `query!` / `query_as!` macros with `$N` bind parameters
- TOCTOU races: check-then-update patterns that should be atomic `UPDATE ... WHERE old_status = $1`
- N+1 queries: missing batch WHERE IN or JOINs for data used in loops
- Missing database constraints (UNIQUE, NOT NULL, CHECK) for invariants enforced only in app code
- `query_as!` without `RETURNING` when the caller needs the updated row

### Race Conditions & Concurrency
- Status transitions without atomic `WHERE old_status` guards — concurrent updates can skip or double-apply
- Missing unique DB index on columns used with `INSERT ... ON CONFLICT`
- Shared mutable state (Arc<Mutex<T>>) with lock held across `.await` points — use tokio::sync::Mutex
- Deadlock potential from acquiring multiple locks in inconsistent order

### Security
- User input reaching SQL without parameterized queries (should never happen with sqlx macros, but check raw queries)
- Missing authorization checks — endpoints that modify resources must validate CurrentUser ownership or role
- Secrets or credentials in source code
- Missing input validation at API boundaries (request DTOs should validate before reaching service layer)

## Review Checklist — INFORMATIONAL (non-blocking)

### Conditional Side Effects
- Code paths that branch on a condition but forget to apply a side effect on one branch
- Log/tracing messages that claim an action happened but the action was conditionally skipped

### Magic Numbers & String Coupling
- Bare numeric literals used in multiple places — should be named constants
- Error message strings used as match targets elsewhere

### Dead Code & Consistency
- Variables assigned but never read
- Comments/docstrings that describe old behavior after the code changed
- Stale TODO comments referencing completed work

### Error Handling
- `unwrap()` or `expect()` in non-test code without provably infallible justification
- Silent error swallowing (matching on Result/Option and discarding the error)
- AppError variants that don't map to the right HTTP status code

### Test Gaps
- Missing negative-path tests for error/rejection cases
- Auth enforcement without integration tests verifying the enforcement path
- State machine transitions without tests for invalid transition attempts
- Domain validation without tests for boundary values

### Performance
- Unbounded queries without LIMIT
- Missing database indexes on columns used in WHERE/JOIN/ORDER BY
- Multiple sequential queries that could be a single JOIN or batch query
- Allocations in hot loops that could be avoided

### Crypto & Entropy
- `rand::thread_rng()` for security-sensitive values — use a CSPRNG
- Non-constant-time comparisons on secrets or tokens (use `subtle::ConstantTimeEq`)
