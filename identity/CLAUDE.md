# Identity Service

Auth, user management, and profile service.

## Data Owned

- Users, Credentials, Roles, Email verification tokens, Password reset tokens

## Architecture

- Layered: `routes` → `service` → `repository` → DB
- All source lives under `src/users/`
- gRPC sidecar for inter-service user lookups

## File Layout

```
identity/
├── Cargo.toml
├── CLAUDE.md
├── migrations/
│   ├── 202602231305_init.sql
│   ├── 202602231752_add_email_verification.sql
│   ├── 202602231807_add_password_reset_tokens.sql
│   └── 202602232000_add_role_check_constraint.sql
├── src/
│   ├── main.rs                    # run_service_with_infra() + gRPC sidecar
│   ├── lib.rs                     # AppState, app(), GetCurrentUser impl
│   └── users/
│       ├── mod.rs
│       ├── routes.rs              # all HTTP handlers
│       ├── service.rs             # business logic (validation, hashing, tokens, caching)
│       ├── repository.rs          # SQL queries (CRUD, token ops)
│       ├── entities.rs            # UserEntity, EmailVerificationTokenEntity, PasswordResetTokenEntity
│       ├── dtos.rs                # request/response DTOs + ValidUserCreateReq, ValidUserUpdateReq
│       ├── value_objects.rs       # Email, Password, Phone, Username
│       └── grpc_service.rs        # GetUser gRPC handler
└── tests/
    ├── integration.rs             # test entry point
    ├── common/mod.rs              # test_db(), test_app_state(), fixture helpers
    └── users/
        ├── mod.rs
        ├── repository_test.rs     # 31 tests
        ├── service_test.rs        # 27 tests
        ├── router_test.rs         # 22 tests
        └── grpc_service_test.rs   # 8 tests
```

## Endpoints (`/api/v1/users`)

**Public:**
| Method | Path | Description |
|--------|------|-------------|
| POST | `/register` | Create user + send verification email |
| POST | `/login` | Authenticate (requires verified email) |
| POST | `/refresh` | Exchange refresh token for new access token |
| POST | `/verify-email` | Verify email with token |
| POST | `/forgot-password` | Request password reset email |
| POST | `/reset-password` | Reset password with token |

**Protected (JWT required):**
| Method | Path | Description |
|--------|------|-------------|
| GET | `/{id}` | Get user (owner or admin) |
| PUT | `/{id}` | Update user (owner or admin) |
| DELETE | `/{id}` | Soft delete user (owner or admin) |
| POST | `/change-password` | Change own password |

## gRPC

- `GetUser(user_id)` → `GetUserResponse { id, username, email, role, email_verified }`
- Proto module: `shared::grpc::identity`

## Entities

- `UserEntity` — id, username, password (argon2), email, phone, role (Buyer/Seller/Admin), email_verified, soft-delete via `deleted_at`
- `EmailVerificationTokenEntity` — token, user_id, expires_at, used_at
- `PasswordResetTokenEntity` — token, user_id, expires_at, used_at

## Value Objects (`src/users/value_objects.rs`)

Input validation via parse-not-validate pattern. Raw `String` fields are validated into typed wrappers at the service boundary.

| Type | Rules                                                                  |
|------|------------------------------------------------------------------------|
| `Email` | RFC 5322 simplified regex, max 254 chars, lowercased                   |
| `Password` | Min 8 chars, requires upper + lower + digit + special                  |
| `Phone` | E.164-ish: `+{cc}-{digits}`, 7-15 total digits                         |
| `Username` | 3-30 chars, `[a-zA-Z0-9_-]`, profanity blocklist (not implemented yet) |

Validated DTOs (`ValidUserCreateReq`, `ValidUserUpdateReq`) are created via `TryFrom` and passed to the repository layer.

## Key Patterns

- **Passwords**: Argon2 hashing
- **Input validation**: Value objects in service layer; repository only accepts validated types
- **Tokens**: 32-byte random hex, 24h expiry (password reset)
- **Caching**: Redis user cache, 5-min TTL, key `user:{uuid}`, evicted on update/delete
- **Transactions**: All writes use `with_transaction()` from shared
- **Auth guards**: `require_access()` — owner or admin; `require_admin()` — admin only
- **Auth middleware**: `AuthMiddleware::new()` with `GetCurrentUser` impl (does DB lookup) — identity is the only service that uses this variant
- **Security**: Silent failure on forgot-password for unknown emails

## Env Vars

| Var | Purpose |
|-----|---------|
| `IDENTITY_DB_URL` | Postgres connection string |
| `IDENTITY_PORT` | HTTP port (default 3000) |
| `IDENTITY_GRPC_PORT` | gRPC port (default 50051) |
| `REDIS_URL` | Redis connection (optional) |
| `ACCESS_TOKEN_SECRET` | JWT access token signing key |
| `REFRESH_TOKEN_SECRET` | JWT refresh token signing key |

## Migrations

Located at `migrations/`, referenced as `./.migrations/identity` at runtime.

## Tests

88 tests across 4 files (31 repository + 27 service + 22 router + 8 gRPC). Run with:
```
make test SERVICE=identity
```
