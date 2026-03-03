# Human Decision TODOs

Items that need a human decision before implementation can proceed.
These are NOT Claude tasks — they require your judgment on approach/tradeoffs.

## Pending Decisions

### API Versioning Strategy
**Context:** Listed in "Patterns to Implement". Need to decide before BFF gateway.
**Options:**
1. URL path prefix (`/v1/products`) — simplest, most visible, easy to route
2. Header-based (`Accept: application/vnd.koupang.v1+json`) — cleaner URLs, harder to test in browser
3. No versioning yet — YAGNI until we have external consumers

**Recommendation:** URL path prefix. It's the simplest, aligns with "tactical first" heuristic, and can be added to the BFF gateway router without touching individual services.

---

### State Machine Approach for Order Service
**Context:** Order lifecycle is the most complex domain logic. Need to decide before building order service.
**Options:**
1. Hand-rolled enum + transition function — full control, no dependency, matches "threshold for abstraction" rule
2. `ruva` crate — event-sourced state machine, aligns with event-driven pattern
3. Typestate pattern — compile-time state transition guarantees, more complex types

**Recommendation:** Start with hand-rolled enum (option 1) — tactical first. If patterns emerge across order/payment/shipping, evaluate `ruva` as the abstraction.

---

### Test Data Builders
**Context:** Each service will need test fixtures. Currently ad-hoc helper functions.
**Options:**
1. Builder pattern per entity (`OrderBuilder::new().with_items(vec![...]).build()`)
2. Factory functions (`test_order()`, `test_order_with_items(n)`)
3. `fake` crate for randomized but realistic data

**Recommendation:** Builder pattern (option 1). Already natural in Rust, composable, self-documenting. Add to `/test-guide` skill when decided.

---

### Migration Conventions
**Context:** Need consistency as services multiply. Currently no explicit convention beyond `NNNN_name.sql`.
**Decisions needed:**
- [ ] Always use `IF NOT EXISTS` for `CREATE TABLE`? (idempotent migrations)
- [ ] Include `DOWN` migrations or forward-only?
- [ ] Index naming convention? (e.g., `idx_{table}_{columns}`)
- [ ] When to add indexes — at table creation or as separate migrations when queries are known?

**Recommendation:** Forward-only (sqlx doesn't have built-in down), `idx_{table}_{columns}` naming, add indexes with the table unless the query pattern isn't known yet.

---

## Patterns to Implement

Aspirational patterns for the project. Decide approach before implementing.

- Api Versioning — see API Versioning Strategy above
- Event Driven Architecture: https://crates.io/crates/ruva
- Transactional Outbox: https://crates.io/crates/outbox-core (already implemented in shared)
- Listen to yourself
- Resilience: https://crates.io/crates/failsafe
- Observability
- Idempotency
- API gateway/BFF
- Background jobs: https://crates.io/crates/aj
- CQRS

## Decided (move here after deciding)

_None yet._
