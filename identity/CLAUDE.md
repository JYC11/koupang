# Identity Service

Auth, user management, and profile service.

## Architecture

- Layered: `routes` → `service` → `domain` → `repository` → DB
- All source under `src/users/`; single domain module
- Rich domain model — `User` with all VO fields, constructed via `TryFrom<UserEntity>`
- Typed IDs via `shared::valid_id!`: `UserId`, `PasswordTokenId`, `EmailTokenId`
- Validated DTOs (`ValidUserCreateReq`, `ValidUserUpdateReq`) via `TryFrom` at service boundary
- gRPC sidecar for inter-service user lookups
- Only service that uses `AuthMiddleware::new()` with DB lookup (others use claims-based)

## File Layout

```
identity/src/
├── main.rs / lib.rs              # AppState, GetCurrentUser impl, gRPC sidecar
└── users/                        # domain.rs, dtos.rs, entities.rs, repository.rs, routes.rs, service.rs, value_objects.rs, grpc_service.rs
```

Migrations: `migrations/` (4 files: init, email verification, password reset, role constraint)
Tests: `tests/users/{repository,service,router,grpc_service}_test.rs` + `tests/common/mod.rs`

## Endpoints (`/api/v1/users`)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/register` | public | Create user + send verification email |
| POST | `/login` | public | Authenticate (requires verified email) |
| POST | `/refresh` | public | Exchange refresh token for new access token |
| POST | `/verify-email` | public | Verify email with token |
| POST | `/forgot-password` | public | Request password reset email (silent on unknown) |
| POST | `/reset-password` | public | Reset password with token |
| GET | `/{id}` | owner/admin | Get user |
| PUT | `/{id}` | owner/admin | Update user |
| DELETE | `/{id}` | owner/admin | Soft delete user |
| POST | `/change-password` | JWT | Change own password |

## gRPC

`GetUser(user_id)` → `{ id, username, email, role, email_verified }` — proto: `shared::grpc::identity`

## Value Objects (`src/users/value_objects.rs`)

| VO | Rules |
|----|-------|
| `Email` | RFC 5322 simplified regex, max 254, lowercased |
| `Password` | Min 8, requires upper + lower + digit + special |
| `Phone` | E.164-ish: `+{cc}-{digits}`, 7-15 total digits |
| `Username` | 3-30 chars, `[a-zA-Z0-9_-]` |

## Key Patterns

- **Passwords:** Argon2 hashing
- **Tokens:** 32-byte random hex, 24h expiry (password reset)
- **Caching:** Redis user cache, 5-min TTL, key `user:{uuid}`, evicted on update/delete
- **Transactions:** All writes use `with_transaction()` from shared
- **Auth guards:** `require_access()` (owner/admin), `require_admin()` (admin only)
- **Soft deletes:** `deleted_at` on users

## Env Vars

`IDENTITY_DB_URL`, `IDENTITY_PORT` (default 3000), `IDENTITY_GRPC_PORT` (default 50051), `REDIS_URL` (optional), `ACCESS_TOKEN_SECRET`, `REFRESH_TOKEN_SECRET`

## Tests

33 unit + 49 integration = 82 tests. `make test SERVICE=identity`

Test layers follow `.plan/test-standards.md`:
- Repository (10): constraint violations, nonexistent entity errors, default values, SQL time filtering
- Service (8): argon2 hashing, JWT validation, GetCurrentUser trait, Redis cache behavior (5 tests)
- Router (25): canonical CRUD flows, HTTP status codes, auth middleware, request parsing
- gRPC (6): protobuf field mapping, error codes
