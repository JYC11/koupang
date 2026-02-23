# Koupang Progress Summary — Part 1 (Identity + Shared)

## Project scaffold

10 crates in a Cargo workspace:
- 1 shared library, 1 BFF gateway, 8 microservices (identity, catalog, order, payment, shipping, notification, review, moderation)
- Each service gets its own Postgres database (created via Docker init script)

## Shared module — the foundation layer

~15 source files providing reusable infrastructure for all services:

- **Service bootstrapper** that wires up DB, Redis, tracing, and optionally gRPC in one call
- **JWT auth system** — access + refresh tokens, middleware, `CurrentUser` extractor, role-based guards (Buyer, Seller, Admin)
- **Transaction support** with savepoint nesting via `with_transaction()` / `with_nested_transaction()`
- **Cursor-based keyset pagination** — `PaginationParams`, `PaginationRes<T>`, generic over any type implementing `HasId`
- **`AppError` enum** that auto-converts to JSON error responses with correct HTTP status codes (400, 401, 403, 404, 409, 500)
- **`EmailService` trait** with a mock implementation that logs instead of sending — ready to swap in a real provider later
- **Protobuf definition** for inter-service identity lookups via gRPC (`GetUser` RPC)
- **Test utilities** using testcontainers — ephemeral Postgres 18 + Redis per test run, HTTP body helpers, gRPC test server
- **Standard API response helpers** — `ok()`, `created()`, `success()`

## Identity service — fully functional auth service

~7 source files, 4 migrations, 88 tests.

### Endpoints

**Public (no auth required):**
- `POST /api/v1/users/register` — create user + send verification email
- `POST /api/v1/users/login` — authenticate (requires verified email)
- `POST /api/v1/users/refresh` — exchange refresh token for new access token
- `POST /api/v1/users/verify-email` — verify email with token
- `POST /api/v1/users/forgot-password` — request password reset email
- `POST /api/v1/users/reset-password` — reset password with token

**Protected (JWT required):**
- `GET /api/v1/users/{id}` — get user (owner or admin)
- `PUT /api/v1/users/{id}` — update user (owner or admin)
- `DELETE /api/v1/users/{id}` — soft delete user (owner or admin)
- `POST /api/v1/users/change-password` — change own password

**gRPC:**
- `GetUser(user_id)` — inter-service user lookups

### Security & patterns

- Argon2 password hashing
- 32-byte random hex tokens with 24-hour expiry (email verification + password reset)
- Redis caching with 5-minute TTL, evicted on writes
- Soft deletes via `deleted_at` column
- Email verification required before login
- Silent forgot-password — doesn't leak whether an email exists
- Layered architecture: routes → service → repository → DB
- All writes wrapped in transactions

### Test coverage

88 tests across 4 test files:
- 31 repository tests (CRUD, duplicates, soft deletes, token lifecycle)
- 27 service tests (auth flows, cache behavior, password operations)
- 22 router/integration tests (full HTTP request/response cycle)
- 8 gRPC service tests

## Infrastructure & tooling

- **Docker Compose** — Postgres 18 + Redis 8.6 with health checks
- **Makefile** — `local-infra`, `local-infra-down`, `test SERVICE=<name>`, `migration SERVICE=<name>`
- **Utility scripts** — test orchestration (`test.sh`), migration scaffolding (`migration.sh`)
- **Planning doc** (`.plan/critical-user-flows.md`) — 8 architectural flows covering ordering saga, shipping, product upload, payment webhooks, registration, browsing/cart/wishlist, returns/refunds, reviews/moderation
