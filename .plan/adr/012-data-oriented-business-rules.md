# ADR-012: Data-Oriented Programming

**Date:** 2026-03-15
**Status:** Accepted

## Context

Inspired by Chris Kiehl's *Data-Oriented Programming in Java* (Manning, Early Access). The book's thesis: "representation is the essence of programming" — programs organized around the data they manage tend to be simpler, smaller, and easier to understand. DOP doesn't replace OOP; the two enhance each other.

### The book's arc (mapped to our codebase)

| Chapter | Concept | Our application |
|---------|---------|----------------|
| 1-2 | Data as values, not mutable objects | Records, immutable value objects (`Price`, `Currency`, `Quantity`) |
| 3 | Data and meaning — semantic types | Validated newtypes (`OrderId`, `IdempotencyKey`, `ShippingAddress`) |
| 4 | Representation — sealed ADTs model domain shape | `OrderStatus` (8 variants), `PaymentState` (7 variants), `Rule<A>` algebra |
| 5 | Behaviors as free functions over data | Service layer as free fns (`create_order()`, `authorize_payment()`), not methods on objects |
| 6 | Implementing domain models — interpreters, exhaustive switches | `transition_to()` match arms, `derive_payment_state()` over transaction list |
| 7 | Guiding design with properties | **Planned**: property-based tests (proptest) for Rule<A> algebraic laws |
| 8 | Business rules as data — Rule ADT + interpreters | `Rule<A>` (`Check \| All \| Any \| Not`) with 6 interpreters |
| 9 | Refactoring towards data | Migrating inline `if`-checks and raw `AppError::BadRequest` to check enums + rule trees |
| 10 | Data-oriented architecture | Our event system: `EventEnvelope` is data, consumers are interpreters, outbox is data-in/data-out |
| 11 | Testing data-oriented programs | Rule tests are pure (plain context struct → predicate → assert), no mocks needed |

### Problems solved

1. **No composability** — validation was scattered inline `if`-checks with no way to aggregate failures or describe the rule set.
2. **Boolean blindness** — `if !condition { return Err(...) }` hides *which* checks failed and *why*.
3. **No per-service error types** — all domain errors funneled through generic `AppError`, losing semantic meaning.

## Decision

### Core DOP Principles (adopted project-wide)

1. **Separate data from behavior.** Data lives in enums, records (structs), value objects. Behavior lives in free functions and interpreters that operate *on* data. No god-objects mixing state + methods.
2. **Data as first-class.** Domain concepts are represented as types — not strings, not i32s, not JSON blobs. `OrderId` not `Uuid`, `Price` not `Decimal`, `OrderStatus::Pending` not `"pending"`.
3. **Interpreters over data.** One data structure, many operations. `Rule<A>` has 6 interpreters (`evaluate`, `evaluate_detailed`, `describe`, `collect_checks`, `collect_failures`, `failure_messages`). Adding a new interpreter doesn't touch existing code. Adding a new variant touches all interpreters (the compiler enforces this via exhaustive match).
4. **Make illegal states unrepresentable.** Validated newtypes at construction boundaries. Sealed enums for state machines. The remaining errors that types can't prevent are handled by `Rule<A>` trees at runtime.
5. **Properties guide the design.** When an algebraic property of your data model is hard to express or frequently violated, the representation is wrong. Let failing properties push toward better types.

### What we built

**`Rule<A>` algebra** (`shared::rules`) — generic composable rule tree:
- Constructors: `Check(A)`, `All(Vec)`, `Any(Vec)`, `Not(Box)`
- Interpreters: `evaluate` (bool), `evaluate_detailed` (result tree), `describe` (human-readable), `collect_checks` (leaf list), `collect_failures` (failed leaves), `failure_messages` (Display strings)

**Per-service error enums** — `OrderError`, `PaymentError`, `CartError` with semantic variants and bidirectional `From` conversions to `AppError`.

**Check enums + rule trees** per service:
- `OrderCheck` → `creation_rules()`, `cancellation_rules()`
- `PaymentCheck` → `authorization_rules()`, `capture_rules()`
- `CheckoutCheck` → `checkout_readiness_rules()`

**Wiring pattern:**
```rust
let ctx = XxxContext::from(&validated);       // plain data, no async
let result = xxx_rules().evaluate_detailed(&eval_xxx(&ctx));
if !result.passed() {
    return Err(XxxError::ValidationFailed(result.failure_messages().join("; ")).into());
}
```

### What we plan to build

**Property-based tests** (proptest) to verify algebraic laws of `Rule<A>`:
- `evaluate` and `evaluate_detailed` agree on pass/fail for all inputs
- `All` with reordered children produces same result (commutativity)
- `describe` never panics for any valid tree
- `collect_failures` returns empty iff `passed() == true`
- Serialization/deserialization round-trips (when rules become config-driven)

These properties will also extend to domain models:
- State machine: terminal states have no valid transitions
- Value objects: construction rejects all out-of-bound values (fuzzing)
- Event processing: `is_event_processed` → handle → `mark_event_processed` is idempotent

## Consequences

**Easier:**
- Adding validation rules: add variant, add to tree, implement predicate.
- Debugging: `evaluate_detailed()` shows exactly which checks failed.
- Testing: context structs are plain data, predicates are pure — no async, no DB, no mocks.
- Documentation: `describe()` auto-generates human-readable rule descriptions.

**Harder:**
- More ceremony for simple 1-3 check validations. Mitigated by keeping direct `if`-guards below the 4-check threshold.
- Developers must learn the check enum + context + predicate + rule tree pattern.

**Unchanged:**
- HTTP response codes — `From<XxxError> for AppError` preserves status codes.
- Value object validation at construction (`Quantity::new()`, `Price::new()`).

## Living Reference

Pattern selection and DOP principles are codified in **STYLE.md § Data-Oriented Programming**. That is the canonical, maintained reference — this ADR records the decision rationale and book mapping.
