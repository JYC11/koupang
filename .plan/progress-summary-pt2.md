# Koupang Progress Summary — Part 2 (Catalog Service)

## Identity service — value object validation (post-pt1)

Added parse-not-validate input validation to the identity service via 4 value object types:
- `Email` — RFC 5322 simplified regex, max 254 chars, lowercased
- `Password` — min 8 chars, requires upper + lower + digit + special character
- `Phone` — E.164-ish format (`+{cc}-{digits}`), 7-15 total digits
- `Username` — 3-30 chars, `[a-zA-Z0-9_-]`

Raw `String` inputs are now validated into typed wrappers at the service boundary. Repository functions only accept validated types (`ValidUserCreateReq`, `ValidUserUpdateReq` via `TryFrom`).

## Catalog service — full product management

~7 source files, 1 migration, 48 tests (28 unit + 20 integration).

### What it covers

Three domain entities managed through a single service layer:

- **Products** — seller-owned listings with name, auto-generated slug, description, base price, currency, category, brand, and a lifecycle status (Draft → Active → Inactive → Archived). Soft-deleted via `deleted_at`.
- **SKUs** — product variants with unique sku codes, individual pricing, stock quantity, and JSONB attributes (e.g. `{"color": "blue", "size": "XL"}`). Soft-deleted via `deleted_at`.
- **Product Images** — URLs with alt text, sort order, and a primary flag. Hard-deleted (no soft delete).

### Endpoints

**Public (no auth):**
- `GET /api/v1/products` — list active products
- `GET /api/v1/products/{id}` — get product with SKUs and images
- `GET /api/v1/products/slug/{slug}` — get product by slug

**Protected (JWT required):**
- `POST /api/v1/products` — create product (sellers only)
- `GET /api/v1/products/seller/me` — list my products
- `PUT /api/v1/products/{id}` — partial update (owner or admin)
- `DELETE /api/v1/products/{id}` — soft delete (owner or admin)
- SKU CRUD: create, update, soft delete, list for product
- Stock adjustment: `POST /skus/{id}/stock` with `{ "delta": N }`
- Image management: add, list, hard delete

### Database schema

Single migration creating 3 tables with:
- `NUMERIC(19,4)` for all prices (ADR-007)
- `JSONB` for SKU attributes
- CHECK constraints on price >= 0, stock >= 0, and status enums
- Partial indexes filtered by `WHERE deleted_at IS NULL` for active-record queries
- Foreign keys: SKUs and images reference products

### Input validation — value objects

Parse-not-validate pattern with 9 value object types:
- `ProductName` (non-empty, max 500, trimmed)
- `Slug` (auto-generated from name, lowercase alphanumeric with hyphens)
- `Price` (Decimal >= 0)
- `SkuCode` (2-100 chars, alphanumeric + hyphens/underscores)
- `StockQuantity` (i32 >= 0)
- `Currency` (3-letter ISO 4217, uppercased)
- `ImageUrl` (http/https, max 2048)
- `ProductStatus` / `SkuStatus` enums

### Test coverage

48 tests across 3 test files + value object unit tests:
- 28 unit tests for value objects (valid/invalid inputs for all 9 types)
- 20 integration tests covering repository CRUD, service logic, and HTTP router
- Follows same testcontainers pattern as identity (ephemeral Postgres 18)

## Shared module enhancements

Three changes to support the catalog service:

1. **Claims-based auth middleware** (`AuthMiddleware::new_claims_based()`) — constructs `CurrentUser` directly from JWT claims without a DB lookup or gRPC call. One-liner setup for downstream services. (ADR-008)
2. **`rust_decimal` support** — enabled the `rust_decimal` feature in sqlx so `Decimal ↔ NUMERIC` round-trips automatically. (ADR-007)
3. **`NoGrpc` type alias** — convenience type for services that don't need a gRPC sidecar (`None::<NoGrpc>`)

## New ADRs

- **ADR-007: Money handling with rust_decimal** — `Decimal` in Rust + `NUMERIC(19,4)` in Postgres. No floating-point rounding bugs. 4 decimal places covers most currencies.
- **ADR-008: Claims-based auth for downstream services** — downstream services validate JWTs from claims alone (zero network/DB calls). Acceptable eventual consistency for short-lived tokens. Future ADR planned for resilient auth with gRPC + cache + circuit breaker.

## Key design decisions in catalog

- **Claims-based auth over gRPC user lookup** — Catalog doesn't own user data, so it trusts the JWT. Simplifies deployment and removes a cross-service dependency.
- **Dynamic partial updates** — `PUT` endpoints use dynamically-built SQL (only updating fields that are present in the request body) rather than requiring all fields.
- **Stock via delta, not absolute** — `POST /skus/{id}/stock` accepts `{ "delta": N }` (positive to add, negative to subtract) rather than setting an absolute value. Safer for concurrent operations.
- **Slug auto-generation** — if no slug is provided, one is generated from the product name (lowercased, spaces to hyphens, non-alphanumeric stripped). Sellers can override with a custom slug.

## Documentation & tooling improvements

- Created CLAUDE.md files for shared, identity, and catalog services
- Added workspace structure tree and ADR summary table to root CLAUDE.md
- Created `project-context` skill for efficient LLM session onboarding and documentation maintenance
- Added prompt logging hook for blogging purposes
