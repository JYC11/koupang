# Catalog Service

Products, pricing, inventory, categories (ltree hierarchy), and brands.

## Architecture

- Layered: `routes` → `service` (free fns) → `domain` → `repository` (free fns) → DB
- Rich domain models — all fields are value objects, constructed via `TryFrom<Entity>`
- Validated DTOs — `ValidCreateProductReq::new(req)` does pure VO construction; FK validation lives in the service layer via `repository::validate_fk_references`
- Typed IDs via `shared::valid_id!`: `ProductId`, `SkuId`, `ProductImageId`, `CategoryId`, `BrandId`
- Name VOs via `shared::validated_name!`: `ProductName(500)`, `CategoryName(255)`, `BrandName(255)`
- Claims-based JWT auth, no gRPC (ADR-008)

## File Layout

```
catalog/src/
├── main.rs / lib.rs              # AppState { pool, cache, auth_config }; Kafka consumer wired via ServiceBuilder
├── common/value_objects.rs       # Slug, HttpUrl (shared across modules)
├── products/                     # 12 endpoints — domain.rs, dtos.rs, entities.rs, repository.rs, routes.rs, service.rs, value_objects.rs
├── categories/                   # 9 endpoints  — same structure as products
├── brands/                       # 9 endpoints  — same structure as products
├── inventory/                    # entities.rs, repository.rs, service.rs — reserve/release/confirm flows
└── consumers/                    # handler.rs (CatalogEventHandler), order_events.rs — consumes orders.events topic
```

Tests: `tests/{products,categories,brands}/{repository,service,router}_test.rs` + `tests/common/mod.rs` (fixtures)

## Endpoints (30 total)

### `/api/v1/products` — owner-or-admin for mutations
| Method | Path | Auth | Notes |
|--------|------|------|-------|
| GET | `/`, `/{id}`, `/slug/{slug}` | public | detail includes SKUs + images; list supports filters |
| POST | `/` | seller | create product |
| GET | `/seller/me` | seller | my products; supports filters + status filter |
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
| `SearchQuery` | products | trimmed, non-empty, max 200 chars (silently truncated) |
| `Currency` | products | 3-letter ISO 4217, uppercased (default: USD) |
| `ProductStatus` | products | Draft, Active, Inactive, Archived (serde: snake_case) |
| `SkuStatus` | products | Active, Inactive, OutOfStock (serde: snake_case) |
| `LtreeLabel` | categories | lowercase + underscores, starts with letter; `from_name("Smart Phones")` → `smart_phones` |

## Key Patterns

- **Money:** `rust_decimal::Decimal` + `NUMERIC(19,4)` (ADR-007)
- **Auth:** `AuthMiddleware::new_claims_based(auth_config)` (ADR-008); `require_access()` for owner/admin
- **Category hierarchy:** Postgres ltree — subtree `<@`, ancestors `@>` (ADR-009)
- **Soft deletes:** Products/SKUs via `deleted_at`; images hard-deleted; categories/brands hard-deleted with guards
- **Partial updates:** `QueryBuilder::separated()` pattern — single if-chain per field (no dual-if-chain)
- **FK validation:** `repository::validate_fk_references(pool, category_id, brand_id)` — existence checks + brand-category association; called from service layer
- **LEFT JOINs:** Product reads JOIN categories/brands for names/slugs in responses
- **Product detail:** Single query with `LEFT JOIN LATERAL` + `json_agg` for product + SKUs + images (1 round trip, not 3); `ProductDetailRow` entity, `ProductDetailRes::from_row()` for JSON deserialization
- **Keyset pagination:** UUID v7-based cursor pagination via `shared::db::pagination_support`; used for `list_brands`, `list_root_categories`, `get_children`, `list_categories_for_brand`; `ProductFilterQuery` → `(PaginationParams, ProductFilter)` via `into_parts()`; supports `cursor`, `limit` (default 20, max 100), `direction` (forward/backward); returns `PaginatedResponse` with `next_cursor` + `prev_cursor`
- **Hard LIMIT safety nets:** `get_subtree` (200), `get_ancestors` (50), `list_skus_by_product` (100), `list_images_by_product` (50) — bounded queries with natural non-ID ordering
- **Bounds:** Stock delta ±10,000; category max depth 10; search max 200 chars
- **List filters:** `category_id`, `brand_id`, `min_price`, `max_price`, `search` (`SearchQuery` VO, ILIKE), `status` (seller/me only)
- **SQL base pattern:** `PRODUCT_LIST_SELECT` ends with `WHERE 1=1`; filters appended as `AND` clauses via `apply_product_filters()` + `keyset_paginate()` via QueryBuilder
- **Existence checks:** `has_products`/`has_children` use `SELECT EXISTS(...)` (short-circuits on first match)

## Inventory Reservations

Saga-integrated inventory management for the order/payment flow:

- **Schema**: `reserved_quantity` column on `skus`, `inventory_reservations` table (order_id + sku_id UNIQUE), `sku_availability` view (available = stock - reserved)
- **Flows**: `reserve_inventory()` (atomic check + increment), `release_reservation()` (cancel path), `confirm_reservation()` (deduct from both stock and reserved)
- **Events**: `InventoryReserved` / `InventoryReservationFailed` written to outbox on `catalog.events` topic
- **Consumer**: `CatalogEventHandler` consumes `orders.events` — `OrderCreated` → reserve, `OrderCancelled` → release
- **Failure path**: If reservation fails, `InventoryReservationFailed` is written on a separate transaction (main tx rolls back)

## Redis Caching

Product read endpoints are cached in Redis (5-minute TTL, gracefully degrades without Redis):

| Endpoint | Cache Key | Evicted By |
|----------|-----------|------------|
| GET `/{id}` (detail) | `product:{uuid}` | update/delete product, create/update/delete SKU, adjust stock, add/delete image |
| GET `/slug/{slug}` | `product:slug:{slug}` | update/delete product |

Lists are **not cached** (paginated + filtered = poor hit rate).

## Env Vars

`CATALOG_DB_URL`, `CATALOG_PORT` (default 3000), `REDIS_URL` (optional), `ACCESS_TOKEN_SECRET`

## Tests

55 unit + 105 integration = 160 tests. `make test SERVICE=catalog`

Test layers follow `/test-guide` skill:
- Products repository (16): internal helpers, JOINs, ltree paths, soft deletes, stock adjustment, CHECK constraints
- Products service (15): business guards, FK validation (6 tests), ownership, hierarchy, status filtering
- Products router (46): canonical CRUD flows, HTTP status codes, auth, pagination, filters
- Products cache (6): Redis cache eviction on mutations
- Inventory repository (16): reserve, release, confirm, availability, insufficient stock, boundaries, multi-order
- Inventory service (6): reserve+outbox, release, confirm

### Tips
- ltree: `::ltree` cast on INSERT, `::text` cast on SELECT
- Fixtures: `create_test_category()`, `create_test_brand()`, `associate_brand_category()` in `tests/common/mod.rs`
- Migrations at `migrations/`, runtime path `./.migrations/catalog`
