# Project

- an ecommerce microservice project for learning and portfolio purposes

## Services

| Service | Status | Responsibility | Data Owned | Docs |
|---------|--------|---------------|------------|------|
| shared | Complete | Shared libraries | — | [shared/CLAUDE.md](shared/CLAUDE.md) |
| identity | Complete (115 tests) | Auth, Users, Profiles | Users, Credentials, Roles | [identity/CLAUDE.md](identity/CLAUDE.md) |
| catalog | Complete (209 tests) | Products, Pricing, Inventory, Categories, Brands | Products, SKUs, Images, Stock, Categories, Brands | [catalog/CLAUDE.md](catalog/CLAUDE.md) |
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
├── .plan/                      # critical-user-flows.md, ADRs (001–009), progress summaries
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

## Reference Skills (auto-triggered, not manually loaded)

- `/bootstrap` — step-by-step for creating a new service
- `/implement` — endpoint, domain layer, and module patterns
- `/test-guide` — what each test layer covers, shared container infrastructure

## Code Review

- **During development**: `/simplify` to review changed code for quality, or ask directly to review specific files/modules
- **Before merge**: `/code-review` on a PR branch for multi-agent review with confidence scoring (requires `gh` CLI)
- **Rust-specific**: rust-skills plugin auto-triggers `m15-anti-pattern`, `coding-guidelines`, `unsafe-checker` when relevant

## Local Infrastructure (docker-compose.infra.yml)

| Service | Image | Host Port | Purpose |
|---------|-------|-----------|---------|
| Postgres | postgres:18 | 5432 | Primary data store |
| Redis | redis:8.6 | 6379 | Cache / session / cart |
| Kafka (KRaft) | apache/kafka:3.9 | 29092 | Event bus |
| Kafka UI | provectuslabs/kafka-ui:0.7 | 8090 | Kafka admin UI |
| Jaeger | jaegertracing/jaeger:2.4 | 16686 (UI), 4317 (OTLP gRPC), 4318 (OTLP HTTP) | Distributed tracing |

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

## Prompt logging

- At the END of every session, log all user prompts to `~/.claude/llm_usage_logging_folder/session-log-YYYY-MM-DD.md`
- This is for blogging purposes — logs will be used in blog posts about LLM usage
- **Format** (append per session):
  ```
  ## Session N — HH:MM

  ### User Prompts
  1. first prompt text
  2. second prompt text

  ### Summary
  One paragraph of what was accomplished.
  ```
- **Do NOT** include: raw JSON, system messages, hook payloads, timestamps per prompt, or code blocks around prompts
- Raw hook data may be appended automatically by hooks — that's fine, but the clean summary section above is what matters for blogging

## Session Start

After loading project context (`/project-context`), always also:
1. Load the `br` skill and run `br list` to see current task state
2. This replaces the need to separately invoke `/br`

## Task management

- beads_rust: https://github.com/Dicklesworthstone/beads_rust
- **Always load the `br` skill (Skill tool) before running any `br` CLI commands.** Do not guess at br syntax — the skill has the complete command reference.
- After plan approval, create br tasks to track implementation work.
