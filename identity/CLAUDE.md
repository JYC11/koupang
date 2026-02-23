# Identity Service

Auth, user management, and profile service.

## Data Owned

- Users, Credentials, Roles, Email verification tokens, Password reset tokens

## Architecture

- Layered: `routes` → `service` → `repository` → DB
- All source lives under `src/users/`
- gRPC sidecar for inter-service user lookups

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

## Key Patterns

- **Passwords**: Argon2 hashing
- **Tokens**: 32-byte random hex, 24h expiry (password reset)
- **Caching**: Redis user cache, 5-min TTL, key `user:{uuid}`, evicted on update/delete
- **Transactions**: All writes use `with_transaction()` from shared
- **Auth guards**: `require_access()` — owner or admin; `require_admin()` — admin only
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

Integration tests in `tests/` covering repository, service, routes, and gRPC layers. Run with:
```
make test SERVICE=identity
```
