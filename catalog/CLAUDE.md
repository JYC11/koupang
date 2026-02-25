# Catalog Service

Products, pricing, inventory, categories (ltree hierarchy), and brands.

## Architecture

- Layered: `routes` → `service` → `domain` → `repository` → DB
- Rich domain models — all fields are value objects, constructed via `TryFrom<Entity>`
- Validated DTOs — `ValidCreateProductReq::new(pool, req)` does VO + async FK validation in one step
- Typed IDs via `shared::valid_id!`: `ProductId`, `SkuId`, `ProductImageId`, `CategoryId`, `BrandId`
- Name VOs via `shared::validated_name!`: `ProductName(500)`, `CategoryName(255)`, `BrandName(255)`
- Claims-based JWT auth, no gRPC (ADR-008)

## File Layout

```
catalog/src/
├── main.rs / lib.rs              # AppState { product_service, category_service, brand_service, jwt_service }
├── common/value_objects.rs       # Slug, HttpUrl (shared across modules)
├── products/                     # 12 endpoints — domain.rs, dtos.rs, entities.rs, repository.rs, routes.rs, service.rs, value_objects.rs
├── categories/                   # 9 endpoints  — same structure as products
└── brands/                       # 9 endpoints  — same structure as products
```

Tests: `tests/{products,categories,brands}/{repository,service,router}_test.rs` + `tests/common/mod.rs` (fixtures)

## Endpoints (30 total)

### `/api/v1/products` — owner-or-admin for mutations
| Method | Path | Auth | Notes |
|--------|------|------|-------|
| GET | `/`, `/{id}`, `/slug/{slug}` | public | detail includes SKUs + images |
| POST | `/` | seller | create product |
| GET | `/seller/me` | seller | my products |
| PUT/DELETE | `/{id}` | owner/admin | soft delete |
| GET/POST | `/{product_id}/skus` | public/owner | list / create SKU |
| PUT/DELETE | `/skus/{sku_id}` | owner/admin | soft delete |
| POST | `/skus/{sku_id}/stock` | owner/admin | `{ "delta": N }` |
| GET/POST/DELETE | `/{product_id}/images[/{image_id}]` | public/owner | hard delete |

### `/api/v1/categories` — admin for mutations
| Method | Path | Auth | Notes |
|--------|------|------|-------|
| GET | `/`, `/{id}`, `/slug/{slug}` | public | root list / by id / by slug |
| GET | `/{id}/children`, `/{id}/subtree`, `/{id}/ancestors` | public | ltree `<@` / `@>` |
| POST/PUT/DELETE | `/[{id}]` | admin | delete guarded: no children, no products |

### `/api/v1/brands` — admin for mutations
| Method | Path | Auth | Notes |
|--------|------|------|-------|
| GET | `/`, `/{id}`, `/slug/{slug}`, `/{id}/categories` | public | |
| POST/PUT/DELETE | `/[{id}]` | admin | delete guarded: no products |
| POST | `/{id}/categories` | admin | associate brand ↔ category |
| DELETE | `/{brand_id}/categories/{category_id}` | admin | disassociate |

## Value Objects

| VO | Location | Rules |
|----|----------|-------|
| `Slug` | common | lowercase + hyphens; `from_name()` auto-generates |
| `HttpUrl` | common | http(s)://, max 2048 |
| `Price` | products | Decimal >= 0 |
| `SkuCode` | products | 2-100 chars, alphanumeric + hyphens/underscores |
| `StockQuantity` | products | i32 >= 0 |
| `Currency` | products | 3-letter ISO 4217, uppercased (default: USD) |
| `ProductStatus` | products | Draft, Active, Inactive, Archived |
| `SkuStatus` | products | Active, Inactive, OutOfStock |
| `LtreeLabel` | categories | lowercase + underscores, starts with letter; `from_name("Smart Phones")` → `smart_phones` |

## Key Patterns

- **Money:** `rust_decimal::Decimal` + `NUMERIC(19,4)` (ADR-007)
- **Auth:** `AuthMiddleware::new_claims_based()` (ADR-008); `require_access()` for owner/admin
- **Category hierarchy:** Postgres ltree — subtree `<@`, ancestors `@>` (ADR-009)
- **Soft deletes:** Products/SKUs via `deleted_at`; images hard-deleted; categories/brands hard-deleted with guards
- **Partial updates:** Dynamic SQL (only provided fields)
- **FK validation:** Validated DTOs enforce FK existence + brand-category association
- **LEFT JOINs:** Product reads JOIN categories/brands for names/slugs in responses

## Env Vars

`CATALOG_DB_URL`, `CATALOG_PORT` (default 3000), `REDIS_URL` (optional), `ACCESS_TOKEN_SECRET`

## Tests

58 unit + 144 integration = 202 tests. `make test SERVICE=catalog`

### Tips
- ltree: `::ltree` cast on INSERT, `::text` cast on SELECT
- Fixtures: `create_test_category()`, `create_test_brand()`, `associate_brand_category()` in `tests/common/mod.rs`
- Migrations at `migrations/`, runtime path `./.migrations/catalog`
