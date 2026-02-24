# Project

- an ecommerce microservice project for learning and portfolio purposes

## Services

| Service | Status | Responsibility | Data Owned | Docs |
|---------|--------|---------------|------------|------|
| shared | Complete | Shared libraries | — | [shared/CLAUDE.md](shared/CLAUDE.md) |
| identity | Complete (88 tests) | Auth, Users, Profiles | Users, Credentials, Roles | [identity/CLAUDE.md](identity/CLAUDE.md) |
| catalog | Complete (48 tests) | Products, Pricing, Inventory | Products, SKUs, Images, Stock | [catalog/CLAUDE.md](catalog/CLAUDE.md) |
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
├── identity/                   # COMPLETE — see identity/CLAUDE.md
├── catalog/                    # COMPLETE — see catalog/CLAUDE.md
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
| 003 | Layered architecture | routes → service → repository |
| 004 | Testcontainers over mocks | Real Postgres 18 / Redis in tests, single-threaded |
| 005 | JWT access + refresh tokens | Stateless access tokens, no DB lookup |
| 006 | Email trait with mock | Decoupled from provider, `MockEmailService` for dev |
| 007 | rust_decimal for money | `Decimal` in Rust, `NUMERIC(19,4)` in Postgres |
| 008 | Claims-based auth downstream | Non-identity services skip user DB lookup |

## Tech Stack

- **Rust**: axum, sqlx, tokio
- **Infra**: Postgres 18 (UUID v7 PKs), Redis
- **Containers**: Docker Compose
- **Observability**: OpenTelemetry, Prometheus
- **Message Queue**: Kafka

## Key Shared Imports

```rust
// Service bootstrap
use shared::server::{run_service_with_infra, ServiceConfig, NoGrpc};
use shared::health::health_routes;

// Auth
use shared::auth::jwt::{JwtService, CurrentUser, AccessTokenClaims, JwtTokens};
use shared::auth::middleware::AuthMiddleware;   // ::new() or ::new_claims_based()
use shared::auth::guards::{require_access, require_admin};
use shared::auth::Role;                         // Buyer, Seller, Admin
use shared::config::auth_config::AuthConfig;

// Database
use shared::db::{PgPool, PgExec, PgConnection};
use shared::db::transaction_support::{with_transaction, with_nested_transaction, TxContext};
use shared::db::pagination_support::{keyset_paginate, get_cursors, PaginationParams, PaginationRes, HasId};
use shared::config::db_config::DbConfig;

// HTTP responses & errors
use shared::errors::AppError;                   // NotFound, Forbidden, Unauthorized, AlreadyExists, InternalServerError, BadRequest
use shared::responses::{ok, success, created};

// Misc
use shared::cache::{init_redis, init_optional_redis};
use shared::dto_helpers::{fmt_id, fmt_datetime, fmt_datetime_opt};
use shared::email::{EmailService, EmailMessage, MockEmailService};

// Test utilities (behind `test-utils` feature)
use shared::test_utils::db::TestDb;
use shared::test_utils::redis::TestRedis;
use shared::test_utils::http::{body_bytes, body_json};
use shared::test_utils::grpc::start_test_grpc_server;
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

## New Service Bootstrap Recipe

Use catalog as the reference implementation:

1. **Add crate to workspace** `Cargo.toml` members list
2. **Create `src/main.rs`** — use `run_service_with_infra()`:
   ```rust
   use <service>::AppState;
   use <service>::app;
   use shared::health::health_routes;
   use shared::server::{NoGrpc, ServiceConfig, run_service_with_infra};

   #[tokio::main]
   async fn main() -> Result<(), Box<dyn Error>> {
       run_service_with_infra(
           ServiceConfig {
               name: "<service>",
               port_env_key: "<SERVICE>_PORT",
               db_url_env_key: "<SERVICE>_DB_URL",
               migrations_dir: "./.migrations/<service>",
           },
           None::<NoGrpc>,  // or Some((GrpcConfig { .. }, grpc_router)) for gRPC
           |pool, redis_conn| {
               let app_state = AppState::new(pool, redis_conn);
               app(app_state).merge(health_routes("<service>"))
           },
       ).await
   }
   ```
3. **Create `src/lib.rs`** — define `AppState` (wraps `Arc<Service>` + `Arc<JwtService>`) and `app()` fn
4. **Create module directory** e.g. `src/orders/` with: `mod.rs`, `routes.rs`, `service.rs`, `repository.rs`, `entities.rs`, `dtos.rs`, `value_objects.rs`
5. **Create first migration**: `make migration SERVICE=<name> NAME=init`
6. **Auth**: use `AuthMiddleware::new_claims_based(jwt_service)` for non-identity services (ADR-008)
7. **Tests**: create `tests/integration.rs`, `tests/common/mod.rs` (with `test_db()`, `test_app_state()`, sample fixtures), and per-module test files
8. **Add CLAUDE.md** in the service directory
9. **Add env vars** to `docker-compose.yml`

## Common Code Patterns

### Adding an endpoint (route → service → repository)

```rust
// routes.rs — define handler, extract state + auth + body
async fn create_thing(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(body): Json<CreateThingReq>,
) -> Result<impl IntoResponse, AppError> {
    let thing = state.service.create_thing(&current_user, body).await?;
    Ok(created("Thing created"))
}

// service.rs — validate inputs, enforce business rules, use transaction
pub async fn create_thing(&self, user: &CurrentUser, req: CreateThingReq) -> Result<ThingDto, AppError> {
    let validated = ValidCreateThingReq::try_from(req)?;
    let entity = with_transaction(&self.pool, |tx| async move {
        ThingRepository::insert(tx.as_executor(), &validated, user.id).await
    }).await?;
    Ok(ThingDto::from(entity))
}

// repository.rs — pure SQL, takes PgConnection for writes, PgExec for reads
pub async fn insert(conn: &mut PgConnection, req: &ValidCreateThingReq, user_id: Uuid) -> Result<ThingEntity, AppError> {
    sqlx::query_as!(ThingEntity, "INSERT INTO things ...")
        .fetch_one(conn).await
        .map_err(|e| AppError::InternalServerError(e.to_string()))
}
```

### Writing an integration test

```rust
// tests/common/mod.rs
pub async fn test_db() -> TestDb {
    TestDb::start("./migrations").await
}
pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_jwt(pool, test_auth_config())
}
pub fn seller_user() -> CurrentUser {
    CurrentUser { id: Uuid::new_v4(), role: Role::Seller }
}

// tests/<module>/<layer>_test.rs
#[tokio::test]
async fn test_create_thing() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let user = seller_user();
    let result = service.create_thing(&user, sample_req()).await;
    assert!(result.is_ok());
}
```

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
