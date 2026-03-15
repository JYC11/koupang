# Style Guide

Design goals, in order: **safety, performance, developer experience**.

Partially adapted from [TigerBeetle's Tiger Style](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md) for Rust.

## Safety

### Assertions and invariants

- **Assert pre/postconditions and invariants.** A function should not operate blindly on data it has not checked. Use `debug_assert!` for invariants that are expensive to check, `assert!` for cheap ones that should hold in production.
- **Pair assertions.** For every property you enforce, find at least two code paths to assert it. Assert data validity before writing to the database AND after reading from the database.
- **Assert positive AND negative space.** Tests must cover valid data, invalid data, and data crossing the valid/invalid boundary.
- **Split compound assertions.** Prefer `assert!(a); assert!(b);` over `assert!(a && b);` — the former gives more precise failure messages.
- **Assert relationships of constants** at compile time via `static_assertions` crate.

### Limits and bounds

- **Put a limit on everything.** All loops, queues, buffers, and retries must have a fixed upper bound. Unbounded anything is a latent production incident.
- Every `Vec` that grows from user input should have a max capacity enforced at the boundary.
- Every retry loop needs `max_retries`. Every timeout needs a `Duration`. Every paginated query needs a `LIMIT`.

### Control flow

- **Simple, explicit control flow.** Minimize nesting depth. No recursion unless the domain is inherently recursive — and even then, bound the depth.
- **Split compound conditions.** `if a && b` makes it hard to verify all cases. Prefer separate guard clauses.
- **State invariants positively.** Prefer `if index < length` (the invariant holds) over `if index >= length` (it doesn't). The former aligns with how the reader thinks about the happy path.
- **Push `if`s up and `for`s down** ([matklad](https://matklad.github.io/2023/11/15/push-ifs-up-and-fors-down.html)). Parent functions own control flow; helpers own computation and are ideally pure.

### Error handling

> "Almost all (92%) of catastrophic system failures are the result of incorrect handling of non-fatal errors explicitly signaled in software." — [Yuan et al., OSDI '14](https://www.usenix.org/system/files/conference/osdi14/osdi14-paper-yuan.pdf)

- **All errors must be handled.** No `let _ = fallible_call();`. If you intentionally discard a Result, comment why.
- **Never silently swallow errors.**
- Domain/service layers return `Result<T, ServiceError>` with per-service error enums (e.g., `PaymentError`).
- `From<ServiceError> for AppError` impl maps domain errors to HTTP responses at the route boundary.
- Infrastructure errors (sqlx, redis) wrapped as `Infra(#[from] AppError)` variant.
- `unwrap`/`expect` only in tests and provably infallible cases (e.g., compiled regex).

### Variables and scope

- **Declare at the smallest possible scope.** Minimize the number of live variables to reduce the probability of using the wrong one.
- **Calculate and check variables close to where they are used.** Don't introduce variables before they are needed. A gap between where a value is computed and where it's consumed is where bugs hide (POCPOU — place-of-check to place-of-use).

### Types and domain modeling

- Favour ADTs, value objects, rich domain models to make illegal states unrepresentable.
- Define errors out of existence — prefer validated newtypes and type states so error cases can't happen; handle the remaining errors that types can't prevent.

## Performance

- **Think about performance from the outset.** The biggest wins (1000x) come from design decisions, not from profiling after the fact.
- **Back-of-envelope sketches.** Before building, estimate against the four resources (network, disk, memory, CPU) and their two characteristics (bandwidth, latency). Sketches are cheap.
- **Optimize for the slowest resource first** (network > disk > memory > CPU), adjusted for frequency. A cache miss that happens 1M times may cost more than a disk fsync that happens once.
- **Batch.** Amortize network, disk, memory, and CPU costs by batching accesses.
- **SQL efficiency.** Favour efficient queries — the less I/O the better.
  - Use LATERAL JOINs with `json_agg`/`json_build_object` to build deduplicated child arrays and avoid cartesian products when joining parent → child entities.
  - Use batch `WHERE IN` to get many if joining is awkward.
  - Do SQL queries the simple way a couple times before finding patterns you can make efficient (same abstraction threshold as code).
  - Caveats: sharding and partitioning can invalidate these patterns.

## Developer Experience

### Function shape

- **Hard limit: 70 lines per function.** There's a sharp discontinuity between a function that fits on a screen and one that requires scrolling. Good splits divide responsibility, not just line count.
  - Good shape is the inverse of an hourglass: few parameters, simple return type, meaty logic in between.
  - Parent function owns control flow (`match`/`if`). Helpers own computation (ideally pure).
  - Parent function keeps state in local variables. Helpers compute what needs to change, not apply the change directly.
- **God functions are bad. Overly fragmented functions are also bad.** Find the balance.
- **Pass-through methods** — a method that only invokes another method and does nothing else — are a smell.

### Naming

| Element | Convention | Example |
|---------|-----------|---------|
| Request/Response DTOs | `VerbNounRequest/Response` | `CreateProductRequest` |
| Service functions | `verb_noun` snake_case | `create_product` |
| Repository functions | `verb_noun` snake_case | `find_product_by_id` |
| Domain models | Plain noun | `Product`, `Brand` |
| Route functions | `verb_noun` snake_case | `create_product`, `get_product_by_id` |
| Modules | Plural noun directories | `products/`, `categories/` |
| Migrations | `NNNN_descriptive_name` | `0001_init.sql` |

- **Add units or qualifiers last, sorted by descending significance.** `latency_ms_max` not `max_latency_ms`. Groups related variables and sorts naturally.
- **Prefer equal-length related names.** `source` and `target` over `src` and `dest` — related variables line up in calculations.
- **Infuse names with meaning.** `pool: PgPool` is fine, but `read_pool: PgPool` and `write_pool: PgPool` are better when both exist.
- **Don't overload names.** If a word means different things in different contexts, pick different words.

### Comments

- **Always say WHY.** Code shows what. Comments explain why you chose this approach over alternatives.
- Comment only things that are not obvious from the code — but when you do comment, make it count.
- **Comments are sentences.** Capital letter, full stop. Space after `//`. Comments after line-end can be phrases without punctuation.
- **Test descriptions.** Write a brief description at the top of non-trivial test functions explaining the goal and methodology.

### Off-by-one awareness

- **Distinguish index, count, and size.** Index is 0-based, count is 1-based, size is count times unit. Mixing them is the #1 source of off-by-one errors.

## Architecture

- **Modules should be deep** — strong functionality behind simple interfaces. Minimize unnecessary information exposed to module users.
- **Different layer, different abstraction.** Each layer in `routes → service → domain → repository` should operate at a distinct level of abstraction. If two adjacent layers use the same vocabulary, one is probably unnecessary.
- **Circular dependencies are bad.** Dependencies should be acyclic and unidirectional.

## Abstraction and complexity

- **Threshold for abstraction: 4+** (DRY principle). 1-3 times, no abstraction (YAGNI). If unsure, repeat a bit more until abstraction becomes obvious.
- **Complexity is incremental.** Each small shortcut compounds. When cleaning up tactical code, fix the small things too.
- **Too much specialization of purpose** can make the code too complicated.
- **Tactical → strategic programming.** In an unsure problem area, employ tactical programming (get things done) to figure out patterns, then clean up with strategic programming (long-term maintenance focus).

## Testing

- **Every bug fix must include a test** — write a regression test that would have caught the bug before fixing the implementation.

## Git workflow

- Commit after completing a logical unit of work (feature, fix, refactor) — not after every file edit.
- **Commit messages are read.** Write descriptive messages. A PR description lives in GitHub, not in `git blame` — the commit message is the permanent record.
- Imperative mood, `type: description` (e.g., `feat: add order state machine`, `fix: prevent double stock reservation`).
- Branch naming: `feat/short-description`, `fix/short-description`, `refactor/short-description`.
- Do not push unless explicitly asked.

## Refactoring

- Refactor in the same PR if <30 min of work; otherwise create a follow-up task.
- Tactical code is fine during exploration; clean up before the feature is "done".

## Escalation

If any of these rules are unclear or conflict, **ESCALATE** — stop and ask the user (or the orchestrating agent in multi-agent setups) before proceeding.

## Attribution

Partially adapted from [TigerBeetle's Tiger Style](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md).
