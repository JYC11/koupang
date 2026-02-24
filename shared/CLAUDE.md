# Shared Crate

Reusable libraries and infrastructure code shared across all microservices.

## File Layout

```
shared/
├── Cargo.toml
├── CLAUDE.md
├── build.rs                       # protobuf compilation
├── proto/
│   └── identity.proto             # GetUser RPC definition
└── src/
    ├── lib.rs                     # re-exports all modules
    ├── server.rs                  # run_service_with_infra(), ServiceConfig, GrpcConfig, NoGrpc
    ├── observability.rs           # init_tracing()
    ├── health.rs                  # health_routes()
    ├── errors.rs                  # AppError enum → IntoResponse
    ├── responses.rs               # ok(), success(), created()
    ├── dto_helpers.rs             # fmt_id(), fmt_datetime(), fmt_datetime_opt()
    ├── auth/
    │   ├── mod.rs
    │   ├── jwt.rs                 # JwtService, CurrentUser, AccessTokenClaims, JwtTokens
    │   ├── middleware.rs          # AuthMiddleware (::new for identity, ::new_claims_based for others)
    │   ├── guards.rs              # require_access(), require_admin()
    │   └── role.rs                # Role enum (Buyer, Seller, Admin)
    ├── db/
    │   ├── mod.rs                 # init_db(), PgPool, PgExec, PgConnection type aliases
    │   ├── transaction_support.rs # TxContext, with_transaction(), with_nested_transaction()
    │   └── pagination_support.rs  # keyset_paginate(), get_cursors(), PaginationParams, PaginationRes, HasId
    ├── config/
    │   ├── mod.rs
    │   ├── db_config.rs           # DbConfig::new(env_key)
    │   ├── auth_config.rs         # AuthConfig::new(), ::for_tests()
    │   ├── redis_config.rs        # RedisConfig::new(), ::try_new()
    │   └── kafka_config.rs        # KafkaConfig { brokers }
    ├── cache/
    │   └── mod.rs                 # init_redis(), init_optional_redis()
    ├── email/
    │   └── mod.rs                 # EmailService trait, EmailMessage, MockEmailService
    ├── grpc/
    │   └── mod.rs                 # grpc::identity (generated protobuf)
    └── test_utils/                # behind `test-utils` feature flag
        ├── mod.rs
        ├── db.rs                  # TestDb::start(migrations_dir) → ephemeral Postgres 18
        ├── redis.rs               # TestRedis::start() → ephemeral Redis
        ├── http.rs                # body_bytes(), body_json()
        └── grpc.rs                # start_test_grpc_server()
```

## Modules

### Bootstrap & Infra (`server`)

- `run_service_with_infra(ServiceConfig, grpc: Option<(GrpcConfig, G)>, build_app)` — full bootstrap (tracing, DB, Redis, TCP, serve); supports optional gRPC sidecar
- `ServiceConfig { name, port_env_key, db_url_env_key, migrations_dir }`
- `GrpcConfig { port_env_key, default_port }`

### Observability (`observability`)

- `init_tracing(service_name)` — tracing subscriber setup, defaults to `{service_name}=debug,tower_http=debug`, respects RUST_LOG

### Health (`health`)

- `health_routes(service_name)` — returns Router with `GET /health` → `{ "status": "ok", "service": "<name>" }`

### Database (`db`)

- `init_db(DbConfig, migrations_dir) -> PgPool` — connect + run migrations
- Type aliases: `PgPool`, `PgExec<'e>` (executor trait)
  - `PgExec<'e>` is used as a type for read operations
  - `PgConnection` is used as a type for write operations

#### Transaction Support (`db::transaction_support`)

- `TxContext` — wraps a transaction, exposes `as_executor()`, `commit()`, `rollback()`, `into_inner()`
- `with_transaction(pool, |tx_ctx| async { ... }) -> TxResult<T>` — wraps operations in a transaction
- `with_nested_transaction(tx_ctx, |tx_ctx| async { ... }) -> TxResult<T>` — savepoint-based nesting
- `TxError` — Database, AlreadyConsumed, Other variants

#### Pagination Support (`db::pagination_support`)

- `keyset_paginate(params, alias, qb)` — appends cursor-based pagination clauses to QueryBuilder
- `get_cursors(params, rows) -> NextAndPrevCursor` — extracts next/prev cursors from results
- `PaginationParams { limit, cursor, direction }` — Forward or Backward
- `PaginationRes<T> { items, next_cursor, prev_cursor }`
- `HasId` trait — implement `fn id(&self) -> Uuid` for paginated types

### Config (`config`)

- `db_config::DbConfig::new(env_key)` — reads DB URL + max connections from env
- `auth_config::AuthConfig::new()` — reads JWT secrets + expiry from env (also `::for_tests()`)
- `kafka_config::KafkaConfig { brokers }` — reads Kafka broker URL
- `redis_config::RedisConfig::new()` — reads REDIS_URL (required); `try_new()` for optional

### Auth (`auth`)

#### Roles (`auth::role`)

- `Role` enum — Buyer, Seller, Admin; derives sqlx::Type, Serialize, Deserialize

#### JWT (`auth::jwt`)

- `JwtService::new(AuthConfig)` — generate/validate access & refresh tokens
- `generate_access_token(user_id, name, role)`, `generate_refresh_token(user_id)`
- `validate_access_token(token) -> AccessTokenClaims`, `validate_refresh_token(token) -> RefreshTokenClaims`
- `refresh_access(refresh_token, name, role) -> String` — validates refresh token and issues new access token
- `CurrentUser { id: Uuid, role: Role }` — also an axum `FromRequestParts` extractor
- `AccessTokenClaims { sub, name, role, iat, exp }` — also an axum `FromRequestParts` extractor
- `JwtTokens { access_token, refresh_token }`

#### Middleware (`auth::middleware`)

- `GetCurrentUser` trait — implement `async fn get_by_id(id: Uuid) -> Result<CurrentUser, AppError>` per service
- `AuthMiddleware::new(Arc<JwtService>, Arc<dyn GetCurrentUser>)` — JWT validation layer with DB lookup (identity service only)
- `AuthMiddleware::new_claims_based(Arc<JwtService>)` — JWT validation from claims only, no DB lookup (all other services, see ADR-008)

#### Guards (`auth::guards`)

- `require_access(current_user, resource_owner_id)` — allows if owner or admin
- `require_admin(current_user)` — allows only admin role

### Cache (`cache`)

- `init_redis(RedisConfig) -> ConnectionManager`
- `init_optional_redis() -> Option<ConnectionManager>` — returns None if REDIS_URL not set

### Email (`email`)

- `EmailMessage { to, subject, body_html }`
- `EmailService` trait — `async fn send_email(message) -> Result<(), AppError>`
- `MockEmailService` — logs emails, always succeeds

### HTTP Responses (`responses`)

- `ok(data)` — wraps in `{ "data": ... }`
- `success(status, message)` — returns `{ "message": "..." }` with given status code
- `created(message)` — shorthand for 201 Created

### Errors (`errors`)

- `AppError` enum — NotFound, Forbidden, Unauthorized, AlreadyExists, InternalServerError, BadRequest
- Implements `IntoResponse` → `{ "error": "..." }` JSON with appropriate status codes

### DTO Helpers (`dto_helpers`)

- `fmt_id(&Uuid) -> String`
- `fmt_datetime(&DateTime<Utc>) -> String` — RFC 3339
- `fmt_datetime_opt(&Option<DateTime<Utc>>) -> Option<String>`

### gRPC (`grpc`)

- `grpc::identity` — generated protobuf module for the identity service

### Test Utilities (`test_utils`, feature: `test-utils`)

- `TestDb::start(migrations_dir) -> TestDb { pool }` — spins up Postgres 18 via testcontainers
- `TestRedis::start() -> TestRedis { conn }` — spins up Redis via testcontainers
- `body_bytes(response) -> Vec<u8>`, `body_json(response) -> serde_json::Value` — HTTP response parsing
- `start_test_grpc_server(router) -> String` — starts gRPC on random port, returns URL

## Key Traits to Implement Per Service

| Trait | Module | Purpose |
|-------|--------|---------|
| `GetCurrentUser` | `auth::middleware` | User lookup for auth middleware (identity only; others use claims-based) |
| `HasId` | `db::pagination_support` | Enables cursor-based pagination |
| `EmailService` | `email` | Email sending (use `MockEmailService` for dev) |
