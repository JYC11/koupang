# Project

- an ecommerce microservice project for learning and portfolio purposes

## Services

| Service | Status | Responsibility | Data Owned | Docs |
|---------|--------|---------------|------------|------|
| shared | Complete | Shared libraries | — | [shared/CLAUDE.md](shared/CLAUDE.md) |
| identity | Complete (115 tests) | Auth, Users, Profiles | Users, Credentials, Roles | [identity/CLAUDE.md](identity/CLAUDE.md) |
| catalog | Complete (202 tests) | Products, Pricing, Inventory, Categories, Brands | Products, SKUs, Images, Stock, Categories, Brands | [catalog/CLAUDE.md](catalog/CLAUDE.md) |
| order | Stub | Order lifecycle (state machine) | Orders, Order Items | — |
| payment | Stub | Payment gateway, wallets | Transactions, Invoices | — |
| shipping | Stub | Logistics, tracking | Shipments, Carriers | — |
| notification | Stub | Emails, SMS, Push | Templates, Delivery Logs | — |
| review | Stub | Product reviews | Reviews | — |
| moderation | Stub | Content moderation | Moderation Log | — |
| bff-gateway | Stub | API gateway | — | — |

## Workspace Structure

```
koupang/
├── Cargo.toml                  # workspace root
├── Makefile
├── docker-compose.yml
├── .plan/                      # critical-user-flows.md, ADRs (001–008), progress summaries
├── shared/                     # COMPLETE — see shared/CLAUDE.md
│   └── src/                    # server, auth/, db/, config/, cache/, email/, errors, responses, test_utils/
├── identity/                   # COMPLETE — see identity/CLAUDE.md IF you are working on identity
├── catalog/                    # COMPLETE — see catalog/CLAUDE.md IF you are working on catalog
├── order/                      # STUB
├── payment/                    # STUB
├── shipping/                   # STUB
├── notification/               # STUB
├── review/                     # STUB
├── moderation/                 # STUB
└── bff-gateway/                # STUB
```

## Documentation

- Update the relevant CLAUDE.md after changes; create ADRs via `make adr` for architectural decisions
- Git tags mark milestones (e.g. `v0.1-identity-auth`); progress summaries live in `.plan/progress-summary-*.md`

## ADR Summary

| # | Decision | Key Implication |
|---|----------|-----------------|
| 001 | Cargo workspace per service | Single `cargo build`, shared deps |
| 002 | UUID v7 primary keys | Time-ordered, good B-tree locality |
| 003 | Layered architecture | routes → service → domain → repository |
| 004 | Testcontainers over mocks | Real Postgres 18 / Redis in tests, single-threaded |
| 005 | JWT access + refresh tokens | Stateless access tokens, no DB lookup |
| 006 | Email trait with mock | Decoupled from provider, `MockEmailService` for dev |
| 007 | rust_decimal for money | `Decimal` in Rust, `NUMERIC(19,4)` in Postgres |
| 008 | Claims-based auth downstream | Non-identity services skip user DB lookup |
| 009 | Postgres ltree for categories | Materialized path hierarchy, `<@`/`@>` tree queries |

## Tech Stack

- **Rust**: axum, sqlx, tokio
- **Infra**: Postgres 18 (UUID v7 PKs), Redis
- **Containers**: Docker Compose
- **Observability**: OpenTelemetry, Prometheus
- **Message Queue**: Kafka

## Key Shared Imports

```rust
use shared::server::{run_service_with_infra, ServiceConfig, NoGrpc};
use shared::auth::jwt::{JwtService, CurrentUser};
use shared::auth::middleware::AuthMiddleware;   // ::new() or ::new_claims_based()
use shared::auth::guards::{require_access, require_admin};
use shared::auth::Role;                         // Buyer, Seller, Admin
use shared::db::{PgPool, PgExec, PgConnection};
use shared::db::transaction_support::{with_transaction, TxContext};
use shared::errors::AppError;                   // NotFound, Forbidden, Unauthorized, AlreadyExists, InternalServerError, BadRequest
use shared::responses::{ok, success, created};
use shared::test_utils::db::TestDb;             // behind `test-utils` feature
```

## Patterns to Implement

- Api Versioning
- Event Driven Architecture: https://crates.io/crates/ruva
- Transactional Outbox: https://crates.io/crates/outbox-core
- Listen to yourself
- Resilience: https://crates.io/crates/failsafe
- Observability
- Idempotency
- API gateway/BFF
- Background jobs: https://crates.io/crates/aj
- CQRS

## Reference Docs (read on-demand, not auto-loaded)

- **[Bootstrap recipe](.plan/bootstrap-recipe.md)** — step-by-step for creating a new service
- **[Code patterns](.plan/patterns.md)** — endpoint, domain layer, and test patterns

## Scripts

- `make run SERVICE=identity` — run a service locally (requires local infra running)
- `make test SERVICE=identity` — run tests for a service
- `make migration SERVICE=identity NAME=init` — create a new migration file
- `make adr` — create a new ADR file (auto-increments number)
- `make local-infra` / `make local-infra-down` — start/stop local Docker infra

## Prompt logging

- At the END of every session, log all user prompts from this session to the memory folder under `llm_usage_logging_folder/`
- Format: append to a file named by date (e.g. `session-log-2026-02-25.md`)
- Each entry should include: session start time, numbered user prompts (just the raw text, no system messages), and a 1-line summary of what was accomplished
- This is for blogging purposes — the logs will be used in blog posts about LLM usage

## Task management

- beads_rust: https://github.com/Dicklesworthstone/beads_rust
  - br skill has been created for use
  - load in this skill first and then create tasks when plan is approved
