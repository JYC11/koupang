# Project

- an ecommerce microservice project for learning and portfolio purposes
- should aim for production quality

## Services

| Service      | Status               | Responsibility                                   | Data Owned                                        | Docs                                     |
| ------------ | -------------------- | ------------------------------------------------ | ------------------------------------------------- | ---------------------------------------- |
| shared       | Complete             | Shared libraries                                 | —                                                 | [shared/CLAUDE.md](shared/CLAUDE.md)     |
| identity     | Complete (84 tests)  | Auth, Users, Profiles                            | Users, Credentials, Roles                         | [identity/CLAUDE.md](identity/CLAUDE.md) |
| catalog      | Complete (161 tests) | Products, Pricing, Inventory, Categories, Brands | Products, SKUs, Images, Stock, Categories, Brands | [catalog/CLAUDE.md](catalog/CLAUDE.md)   |
| order        | Complete (88 tests)  | Order lifecycle (state machine)                  | Orders, Order Items                               | [order/CLAUDE.md](order/CLAUDE.md)       |
| payment      | Complete (98 tests)  | Payment gateway, double-entry ledger             | Accounts, Transactions, Entries                   | [payment/CLAUDE.md](payment/CLAUDE.md)   |
| saga-tests   | Complete (6 tests)   | Cross-service saga integration tests             | —                                                 | —                                        |
| cart         | Complete (59 tests)  | Shopping cart (Redis)                             | Cart Items (Redis hash)                           | [cart/CLAUDE.md](cart/CLAUDE.md)         |
| shipping     | Stub                 | Logistics, tracking                              | Shipments, Carriers                               | —                                        |
| notification | Stub                 | Emails, SMS, Push                                | Templates, Delivery Logs                          | —                                        |
| review       | Stub                 | Product reviews                                  | Reviews                                           | —                                        |
| moderation   | Stub                 | Content moderation                               | Moderation Log                                    | —                                        |
| bff-gateway  | Stub                 | API gateway                                      | —                                                 | —                                        |

## Documentation

- Update the relevant CLAUDE.md after changes; create ADRs via `make adr` for architectural decisions
- Git tags mark milestones (e.g. `v0.1-identity-auth`); progress summaries live in `.plan/progress-summary-*.md`
- Reference docs: [docs/ordering-saga-flows.md](docs/ordering-saga-flows.md) (saga event flows), [docs/OUTBOX_LIFECYCLE.md](docs/OUTBOX_LIFECYCLE.md) (outbox relay lifecycle), [docs/PERSISTENT_JOB_LIFECYCLE.md](docs/PERSISTENT_JOB_LIFECYCLE.md) (persistent job lifecycle), [docs/adr/](docs/adr/) (ADRs)

## ADR Summary

| #   | Decision                      | Key Implication                                     |
| --- | ----------------------------- | --------------------------------------------------- |
| 001 | Cargo workspace per service   | Single `cargo build`, shared deps                   |
| 002 | UUID v7 primary keys          | Time-ordered, good B-tree locality                  |
| 003 | Layered architecture          | routes → service (free fns) → domain → repository   |
| 004 | Testcontainers over mocks     | Real Postgres 18 / Redis in tests, single-threaded  |
| 005 | JWT access + refresh tokens   | Stateless access tokens, no DB lookup               |
| 006 | Email trait with mock         | Decoupled from provider, `MockEmailService` for dev |
| 007 | rust_decimal for money        | `Decimal` in Rust, `NUMERIC(19,4)` in Postgres      |
| 008 | Claims-based auth downstream  | Non-identity services skip user DB lookup           |
| 009 | Postgres ltree for categories | Materialized path hierarchy, `<@`/`@>` tree queries |
| 010 | Inter-service communication   | REST for queries, Kafka events for state changes    |
| 011 | Event schema conventions      | `{svc}.{entity}.{verb}` naming, versioned envelope  |
| 012 | Data-oriented programming     | DOP principles, `Rule<A>` algebra, per-service errors, property testing |

## Tech Stack

- **Rust**: axum, sqlx, tokio
- **Infra**: Postgres 18 (UUID v7 PKs), Redis
- **Containers**: Docker Compose
- **Observability**: OpenTelemetry, Prometheus
- **Message Queue**: Kafka

## Key Shared Imports

```rust
use shared::server::{ServiceBuilder, Infra, GrpcConfig, ConsumerRegistration};
use shared::auth::jwt::{self, CurrentUser};     // jwt:: free functions (generate_access_token, validate_access_token, etc.)
use shared::auth::middleware::AuthMiddleware;   // ::new(auth_config, getter) or ::new_claims_based(auth_config)
use shared::auth::guards::{require_access, require_admin};
use shared::auth::Role;                         // Buyer, Seller, Admin
use shared::config::auth_config::AuthConfig;    // passed to jwt:: functions and AuthMiddleware
use shared::db::{PgPool, PgExec, PgConnection};
use shared::db::transaction_support::{with_transaction, TxContext};
use shared::errors::AppError;                   // NotFound, Forbidden, Unauthorized, AlreadyExists, InternalServerError, BadRequest
use shared::responses::{ok, success, created};
use shared::rules::{Rule, RuleResult};           // composable rule algebra (ADR-012)
use shared::new_types::money::{Price, Currency, Money}; // shared money VOs
use shared::test_utils::db::TestDb;             // behind `test-utils` feature
use shared::test_utils::events::make_envelope;  // test envelope builder (auto source/aggregate)
use shared::distributed_lock::DistributedLock;  // Redis SETNX + Lua atomic release
use shared::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, BreakerStatus}; // generic circuit breaker
```

## Patterns to Implement

See `.plan/human-todos.md` for full list with crate links and decision status.

## Development Rules

All coding style, naming, error handling, git workflow, and architectural heuristics live in **[STYLE.md](STYLE.md)**. Read it before writing code.

## Reference Skills (auto-triggered, not manually loaded)

- `/bootstrap` — step-by-step for creating a new service
- `/implement` — endpoint, domain layer, and module patterns
- `/test-guide` — what each test layer covers, shared container infrastructure

## Code Review

- **During development**: `/simplify` to review changed code for quality, or ask directly to review specific files/modules
- **Before merge**: `/code-review` on a PR branch for multi-agent review with confidence scoring (requires `gh` CLI)
- **Rust-specific**: rust-skills plugin auto-triggers `m15-anti-pattern`, `coding-guidelines`, `unsafe-checker` when relevant

## Scripts

- `make fmt SERVICE=identity` — format a service (`CHECK=1` for CI check-only mode)
- `make check SERVICE=identity` — type-check a service (`CLIPPY=1` to also run clippy)
- `make build SERVICE=identity` — build a service (`RELEASE=1` for release mode)
- `make run SERVICE=identity` — run a service locally (requires local infra running)
- `make test SERVICE=identity` — run tests for a service
- `make migration SERVICE=identity NAME=init` — create a new migration file
- `make adr` — create a new ADR file (auto-increments number)
- `make local-infra` / `make local-infra-down` — start/stop local Docker infra

All service commands accept `SERVICE=all` to run against every service. The scripts handle the `shared` crate's `--features test-utils` flag automatically.

## Prompt Logging

- At session END, append to `~/.claude/llm_usage_logging_folder/session-log-YYYY-MM-DD.md`
- Format: `## Session N — HH:MM` header, numbered user prompts, one-paragraph summary
- Omit: raw JSON, system messages, hook payloads, code blocks around prompts

## Session & Task Management

- At session start: load `/project-context`, then load `/filament` skill and run `fl task ready`
- After plan approval, create `fl` tasks to track work
- Filament reference: https://github.com/JYC11/filament
