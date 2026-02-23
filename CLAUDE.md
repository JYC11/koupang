# Project

- an ecommerce microservice project for learning and portfolio purposes

## Features

- refer to .plan/critical-user-flows.md for user flows as needed

## Microservices

- Identity
  - Responsibility: Auth, Users, Profiles
  - Data owned: Users, Credentials, Roles
- Catalog
  - Responsibility: Product Info, Pricing, Inventory
  - Data owned: Products, stock levels
- Order
  - Responsibility: Order lifcycle (state machine)
  - Data owned: Orders, order items
- Payment
  - Responsibility: Payament gateway integration, wallets
  - Data owned: Transactions, Invoices
- Shipping
  - Responsibility: Logistics, tracking, addresses
  - Data owned: Shipments, Carriers
- Notification
  - Responsibility: Emails, SMS, Push
  - Data owned: Templates, Delivery Logs
- Review
  - Responsibility: Product reviews
  - Data owned: Review
- Moderation
  - Responsiblity: Moderating seller products and buyer reviews
  - Data owned: Moderation log
- Shared
  - Responsibility: contains shared libraries/code between services
  - Reusable modules (use these when building new services):
    - Bootstrap & Infra:
      - `server::run_service(ServiceConfig, |pool, common_state| Router)` — full bootstrap (tracing, DB, TCP, serve)
      - `server::ServiceConfig { name, db_url_env_key, migrations_dir }`
      - `CommonAppState::new()` — reads PORT env var (default 3000)
      - `observability::init_tracing(service_name)` — tracing subscriber setup, respects RUST_LOG
      - `health::health_routes(service_name)` — `GET /health` returning `{ status, service }` JSON
    - Database (`db`):
      - `init_db(DbConfig, migrations_dir) -> PgPool` — connect + run migrations
      - Types: `PgPool`, `PgTx<'a>`, `PgExec<'e>` (executor trait)
      - `transaction_support::with_transaction(pool, |tx_ctx| async { ... })` — wraps operations in a transaction
      - `transaction_support::with_nested_transaction(tx_ctx, |tx_ctx| async { ... })` — savepoint-based nesting
      - `pagination_support::keyset_paginate(params, alias, qb)` — appends pagination clauses to QueryBuilder
      - `pagination_support::get_cursors(params, rows)` — extracts next/prev cursors from results
      - `pagination_support::PaginationParams { limit, cursor, direction }`, `PaginationRes<T>`, `HasId` trait
    - Config (`config`):
      - `db_config::DbConfig::new(env_key)` — reads DB URL + max connections from env
      - `auth_config::AuthConfig::new()` — reads JWT secrets + expiry from env (also `::for_tests()`)
      - `kafka_config::KafkaConfig` — reads Kafka broker URL
      - `redis_config::RedisConfig` — reads Redis URL
    - Auth (`auth`):
      - `jwt::JwtService::new(AuthConfig)` — generate/validate access & refresh tokens
      - `jwt::CurrentUser { id, role }` — also an axum extractor via FromRequestParts
      - `jwt::AccessTokenClaims` — also an axum extractor
      - `jwt::JwtTokens { access_token, refresh_token }`
      - `middleware::AuthMiddleware::new(Arc<JwtService>, Arc<dyn GetCurrentUser>)` — JWT validation layer
      - `middleware::GetCurrentUser` trait — implement `async fn get_by_id(id) -> Result<CurrentUser>` per service
      - `guards::require_access(&current_user, &resource_id)`, `require_admin(&current_user)` — authorization checks
    - HTTP Responses:
      - `responses::ok(data)`, `responses::success(status, msg)`, `responses::created(msg)` — standardized JSON responses
      - `errors::AppError` — variants: NotFound, Forbidden, Unauthorized, AlreadyExists, InternalServerError, BadRequest; returns `{ "error": "..." }` JSON
    - DTO Helpers:
      - `dto_helpers::fmt_id(&Uuid)`, `fmt_datetime(&DateTime<Utc>)`, `fmt_datetime_opt(&Option<DateTime<Utc>>)` — RFC 3339 formatting

## Tech stack

- rust (most used crates in no particular order)
  - axum
  - sqlx
  - tokio
- infra
  - postgres
    - using version 18
    - using uuid v7 as a primary key
  - redis
- containerization
  - docker
  - docker compose
- monitoring/observability
  - opentelemetry
  - prometheus
- message queue
- kafka

## Patterns to implement

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

## Scripts
- refer to the Makefile
  - has scripts for running tests and creating empty migration files

## Task management

- beads_rust: https://github.com/Dicklesworthstone/beads_rust
  - br skill has been created for use
