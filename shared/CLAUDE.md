# Shared Crate

Reusable libraries and infrastructure code shared across all microservices.

## File Layout

```
shared/src/
├── lib.rs                     # re-exports all modules
├── server.rs                  # run_service_with_infra(), ServiceConfig, GrpcConfig, NoGrpc
├── observability.rs           # init_tracing()
├── health.rs                  # health_routes() → GET /health
├── errors.rs                  # AppError enum → IntoResponse
├── responses.rs               # ok(), success(), created()
├── dto_helpers.rs             # fmt_id(), fmt_datetime(), fmt_datetime_opt()
├── auth/
│   ├── jwt.rs                 # JwtService, CurrentUser, AccessTokenClaims, JwtTokens
│   ├── middleware.rs          # AuthMiddleware (::new for identity, ::new_claims_based for others)
│   ├── guards.rs              # require_access(), require_admin()
│   └── role.rs                # Role enum (Buyer, Seller, Admin)
├── db/
│   ├── mod.rs                 # init_db(), PgPool, PgExec, PgConnection
│   ├── transaction_support.rs # TxContext, with_transaction(), with_nested_transaction()
│   └── pagination_support.rs  # keyset_paginate(), get_cursors(), PaginationParams (Default), PaginationRes, HasId
├── config/
│   ├── db_config.rs           # DbConfig::new(env_key)
│   ├── auth_config.rs         # AuthConfig::new(), ::for_tests()
│   ├── redis_config.rs        # RedisConfig::new(), ::try_new()
│   └── kafka_config.rs        # KafkaConfig { brokers }
├── cache/mod.rs               # init_redis(), init_optional_redis()
├── email/mod.rs               # EmailService trait, EmailMessage, MockEmailService
├── grpc/mod.rs                # grpc::identity (generated protobuf)
└── test_utils/                # behind `test-utils` feature
    ├── auth.rs                # test_auth_config(), test_token(), seller_user(), buyer_user(), admin_user()
    ├── http.rs                # body_bytes(), body_json(), json_request(), authed_json_request(), authed_get(), authed_delete()
    ├── db.rs                  # TestDb::start(migrations_dir)
    ├── redis.rs               # TestRedis::start()
    └── grpc.rs                # start_test_grpc_server()
```

## Key APIs

| Module | Key exports |
|--------|-------------|
| `server` | `run_service_with_infra(ServiceConfig, grpc, build_app)` — full bootstrap |
| `db` | `init_db()`, `PgPool`, `PgExec<'e>` (reads), `PgConnection` (writes) |
| `db::transaction_support` | `with_transaction(pool, closure)`, `with_nested_transaction(tx, closure)`, `TxContext` |
| `db::pagination_support` | `keyset_paginate(params, alias, qb)`, `get_cursors(params, rows)`, `PaginationParams` (impl `Default`: limit=20, forward), `PaginationRes<T>`, `HasId` trait |
| `auth::jwt` | `JwtService::new(AuthConfig)`, `CurrentUser { id, role }` (axum extractor), `AccessTokenClaims` (axum extractor) |
| `auth::middleware` | `AuthMiddleware::new(jwt, user_lookup)` (identity), `::new_claims_based(jwt)` (other services, ADR-008) |
| `auth::guards` | `require_access(user, owner_id)`, `require_admin(user)` |
| `auth::role` | `Role` — Buyer, Seller, Admin |
| `config` | `DbConfig`, `AuthConfig`, `RedisConfig` (`.new()` / `.try_new()`), `KafkaConfig` |
| `errors` | `AppError` — NotFound, Forbidden, Unauthorized, AlreadyExists, InternalServerError, BadRequest |
| `responses` | `ok(data)`, `success(status, msg)`, `created(msg)` |
| `email` | `EmailService` trait, `MockEmailService` |

## Test Utilities (feature: `test-utils`)

| Helper | Purpose |
|--------|---------|
| `test_utils::auth::test_auth_config()` | Deterministic AuthConfig (3600s access, 7200s refresh) |
| `test_utils::auth::test_token(user)` | JWT access token for a `CurrentUser` |
| `test_utils::auth::{seller,buyer,admin}_user()` | `CurrentUser` with random UUID and role |
| `test_utils::http::body_bytes/body_json` | Parse response body |
| `test_utils::http::json_request` | Unauthenticated JSON request builder |
| `test_utils::http::authed_json_request` | Authenticated JSON request builder |
| `test_utils::http::authed_get/authed_delete` | Authenticated GET/DELETE builders |
| `test_utils::db::TestDb::start(dir)` | Shared Postgres 18 container; per-test DB via `CREATE DATABASE ... TEMPLATE` |
| `test_utils::redis::TestRedis::start()` | Shared Redis container; `FLUSHDB` per test for isolation |

## Key Traits to Implement Per Service

| Trait | Module | When |
|-------|--------|------|
| `HasId` | `db::pagination_support` | Any paginated entity — `fn id(&self) -> Uuid` |
| `GetCurrentUser` | `auth::middleware` | Identity service only (others use claims-based) |
| `EmailService` | `email` | If service sends emails (use `MockEmailService` for dev) |
