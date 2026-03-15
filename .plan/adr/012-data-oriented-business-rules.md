# ADR-012: Data-Oriented Business Rules

**Date:** 2026-03-15
**Status:** Accepted

## Context

Business validation logic was scattered across service functions as inline `if`-checks and `match` arms, returning raw `AppError::BadRequest(String)`. This had three problems:

1. **No composability** — each validation was an independent check with no way to aggregate failures or describe the rule set.
2. **Boolean blindness** — `if !condition { return Err(...) }` hides *which* checks failed and *why*.
3. **No per-service error types** — all domain errors funneled through the generic `AppError` enum, losing semantic meaning at the service boundary.

Inspired by the DOP (Data-Oriented Programming) pattern (Ch. 3, 6, 8): represent validation logic as algebraic data types with multiple interpreters.

## Decision

### 1. Generic Rule Algebra (`shared::rules`)

The existing `Rule<A>` algebra (`Check | All | Any | Not`) with interpreters (`evaluate`, `evaluate_detailed`, `describe`, `collect_checks`) was extended with:

- **`collect_failures()`** on `RuleResult<A>` — recursively collects all failed leaf checks.
- **`failure_messages()`** on `RuleResult<A: Display>` — maps failures to strings for error reporting.

These bridge the rule system to error responses: `evaluate_detailed() → collect_failures() → error message`.

### 2. Per-Service Error Enums

Each service defines a domain error enum that maps to HTTP responses via `From<XxxError> for AppError`:

| Service | Error Enum | Key Variants |
|---------|-----------|-------------|
| Order | `OrderError` | `ValidationFailed`, `InvalidTransition{from,to}`, `CancellationDenied` |
| Payment | `PaymentError` | `ValidationFailed`, `InvalidState{operation,state}`, `AmountTampered{requested,approved}` |
| Cart | `CartError` | `ValidationFailed`, `CartFull{max}`, `ItemNotFound`, `CheckoutNotReady` |

### 3. Check Enums + Rule Trees

Each service defines a check enum (the `A` in `Rule<A>`) and composable rule trees:

- **`OrderCheck`** → `creation_rules()`, `cancellation_rules()`
- **`PaymentCheck`** → `authorization_rules()`, `capture_rules()`
- **`CheckoutCheck`** → `checkout_readiness_rules()`

Each check enum implements `Display` for human-readable error messages. Predicate functions (`eval_creation`, `eval_checkout`, etc.) evaluate checks against a context struct.

### 4. Wiring Pattern

```rust
let ctx = XxxContext::from(...);
let rules = xxx_rules();
let result = rules.evaluate_detailed(&eval_xxx(&ctx));
if !result.passed() {
    return Err(XxxError::ValidationFailed(result.failure_messages().join("; ")).into());
}
```

### 5. Threshold Constants

Business thresholds are hardcoded `const` in each `rules.rs` (e.g., `MIN_ORDER_AMOUNT = $1.00`, `MAX_CART_ITEMS = 50`, `MIN_PAYMENT_AMOUNT = $0.50`). Simple, testable, and easy to migrate to config if needed.

## Consequences

**Easier:**
- Adding new validation rules: add a variant to the check enum, add it to the rule tree, implement the predicate — done.
- Debugging failures: `evaluate_detailed()` tells you exactly which checks failed and why.
- Testing rules: context structs are plain data, rule trees are pure functions — no async, no DB, no mocking.
- Describing rules: `describe()` produces human-readable rule documentation automatically.

**Harder:**
- Simple single-check validations now have more ceremony. Mitigated by keeping direct `if`-checks for cases below 4 checks (per STYLE.md guidance).
- Developers must learn the `Rule<A>` + context + predicate pattern. Mitigated by consistent structure across all 3 services.

**Unchanged:**
- HTTP response codes — `From<XxxError> for AppError` preserves the same status codes.
- Existing value object validation — `Quantity::new()`, `Price::new()` etc. still validate at construction time.

## Living Reference

Pattern selection guidance (when to use `if`-guards vs `Rule<A>` vs runtime enum vs typestate) is codified in **STYLE.md § Validation pattern selection**. That is the canonical, maintained reference — this ADR records the decision rationale.
