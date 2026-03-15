# Style Guide

Design goals, in order: **safety, performance, developer experience**.

Adapted from [TigerBeetle's Tiger Style](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md) for Rust.

## Safety

### Assertions and invariants

- **Assert pre/postconditions and invariants.** Use `debug_assert!` for expensive checks, `assert!` for cheap production checks.
- **Pair assertions.** Assert data validity before writing to DB AND after reading from DB.
- **Assert positive AND negative space.** Tests must cover valid, invalid, and boundary data.
- **Split compound assertions.** `assert!(a); assert!(b);` over `assert!(a && b);` for precise failures.

### Limits and bounds

- **Put a limit on everything.** Loops, queues, buffers, retries — all need a fixed upper bound. Every `Vec` from user input needs a max capacity. Every retry needs `max_retries`. Every timeout needs a `Duration`. Every paginated query needs a `LIMIT`.

### Control flow

- **Simple, explicit control flow.** Minimize nesting. No recursion unless inherently recursive (and bounded).
- **Split compound conditions.** Prefer separate guard clauses over `if a && b`.
- **State invariants positively.** `if index < length` (holds) over `if index >= length` (doesn't).
- **Push `if`s up and `for`s down.** Parent functions own control flow; helpers own computation.

### Error handling

- **All errors must be handled.** No `let _ = fallible_call();` without a comment explaining why.
- `unwrap`/`expect` only in tests and provably infallible cases (e.g., compiled regex).
- Per-service error enums and `From` conversions: see *Per-service error enums* below.

### Variables and scope

- **Declare at the smallest possible scope.** Fewer live variables = fewer bugs.
- **Calculate and check close to use.** Gap between computation and consumption is where bugs hide (POCPOU).

### Data-Oriented Programming (ADR-012)

Adapted from Chris Kiehl's *Data-Oriented Programming in Java*. Core principles:

1. **Separate data from behavior.** Data lives in enums, structs, value objects. Behavior lives in free functions and interpreters over data. No god-objects mixing state + methods.
2. **Data as first-class.** Domain concepts are types — `OrderId` not `Uuid`, `Price` not `Decimal`, `OrderStatus::Pending` not `"pending"`. Validated newtypes make illegal states unrepresentable.
3. **One data structure, many interpreters.** `Rule<A>` has 6 interpreters. Adding an interpreter doesn't touch existing code. Adding a variant touches all interpreters (compiler-enforced exhaustive match).
4. **Properties guide the design.** When an algebraic property is hard to express, the representation is wrong. Use property-based tests (proptest) to verify laws and let failures push toward better types.

### Validation patterns

Pick the simplest tool. Escalate only when simpler doesn't work.

| # of checks | Pattern | Example |
|-------------|---------|---------|
| 1-3 | Direct `if`-guards | `add_item()`: single `count >= MAX` check |
| 4+ | `Rule<A>` tree | `checkout_readiness_rules()`: 6 composable checks |
| State machine | Runtime enum + `transition_to()` | `OrderStatus`, `PaymentState` |
| In-memory linear (2-4 states) | Typestate | Builder pattern |

**`Rule<A>` trees** — each service defines: a check enum with `Display`, a context struct (plain data), a pure predicate fn, and rule trees. Thresholds as `const` in `rules.rs`. Wiring:

```rust
let ctx = XxxContext::from(&validated);
let result = xxx_rules().evaluate_detailed(&eval_xxx(&ctx));
if !result.passed() {
    return Err(XxxError::ValidationFailed(result.failure_messages().join("; ")).into());
}
```

**Runtime enum state machines** — for DB-persisted or event-derived state. `transition_to()` with allowed-transition tables. Do NOT use typestate when: state comes from DB/events, external systems drive transitions, 5+ states, or cancellation spans multiple states.

### Property-based testing

Use proptest to verify algebraic laws — these catch design flaws that example-based tests miss:

- **Rule algebra**: `evaluate` and `evaluate_detailed` agree on pass/fail; `All` is order-independent; `collect_failures` is empty iff `passed()`.
- **State machines**: terminal states have no valid transitions; transition graph has no unreachable states.
- **Value objects**: construction rejects all out-of-bound values (fuzz the boundaries).
- **Event processing**: handle + mark_processed is idempotent.

When a property is hard to state or frequently violated, the representation is wrong — fix the types, not the test.

### Per-service error enums

Each service owns its error type (`OrderError`, `PaymentError`, `CartError`) with semantic variants (`ValidationFailed`, `InvalidTransition`, `NotFound`, `Infra(AppError)`, etc.). `From<AppError> for ServiceError` wraps infra inward. `From<ServiceError> for AppError` maps to HTTP at the boundary. Domain logic never returns raw `AppError::BadRequest(String)`.

## Performance

- **Think about performance from the outset.** The biggest wins (1000x) come from design, not profiling.
- **Back-of-envelope sketches** against network, disk, memory, CPU (bandwidth + latency).
- **Optimize for the slowest resource first** (network > disk > memory > CPU), adjusted for frequency.
- **Batch.** Amortize costs by batching accesses.
- **SQL efficiency.** LATERAL JOINs with `json_agg` for parent→child without cartesian products. Batch `WHERE IN` when joins are awkward. Start simple, optimize when patterns emerge. Caveats: sharding/partitioning can invalidate these.

## Developer Experience

### Function shape

- **Hard limit: 70 lines per function.** Good splits divide responsibility, not just line count.
  - Few parameters, simple return type, meaty logic in between.
  - Parent owns control flow. Helpers own computation (ideally pure).
- **God functions and overly fragmented functions are both bad.** Find the balance.
- **Pass-through methods** (method that only invokes another) are a smell.

### Naming

| Element | Convention | Example |
|---------|-----------|---------|
| DTOs | `VerbNounReq/Res` | `CreateProductReq` |
| Service/repository/route fns | `verb_noun` snake_case | `create_product` |
| Domain models | Plain noun | `Product`, `Brand` |
| Modules | Plural noun directories | `products/`, `categories/` |
| Migrations | `NNNN_descriptive_name` | `0001_init.sql` |

- **Units/qualifiers last**, descending significance: `latency_ms_max` not `max_latency_ms`.
- **Equal-length related names.** `source`/`target` over `src`/`dest`.
- **Infuse names with meaning.** `read_pool`/`write_pool` over two generic `pool` variables.

### Comments

- **Always say WHY**, not what. Comment only non-obvious things — but make those count.
- **Comments are sentences.** Capital letter, full stop. Space after `//`. Line-end comments can be phrases.

### Off-by-one awareness

- **Distinguish index (0-based), count (1-based), and size (count * unit).** Mixing them is the #1 off-by-one source.

## Architecture

- **Modules should be deep** — strong functionality behind simple interfaces.
- **Different layer, different abstraction.** `routes → service → domain → repository` each at a distinct level. If two adjacent layers share vocabulary, one is probably unnecessary.
- **No circular dependencies.** Acyclic, unidirectional.

## Abstraction and complexity

- **Threshold for abstraction: 4+.** 1-3 times, just repeat (YAGNI). If unsure, repeat more until the abstraction is obvious.
- **Complexity is incremental.** Each shortcut compounds. When cleaning up, fix the small things too.
- **Tactical → strategic.** Explore with tactical code, then clean up before "done".

## Testing

- **Every bug fix must include a regression test** that would have caught the bug.

## Git workflow

- Commit after completing a logical unit of work, not after every file edit.
- **Commit messages are read.** Imperative mood, `type: description` (e.g., `feat: add order state machine`).
- Branch naming: `feat/short-description`, `fix/short-description`, `refactor/short-description`.
- Do not push unless explicitly asked.

## Escalation

If any of these rules are unclear or conflict, **ESCALATE** — stop and ask before proceeding.
