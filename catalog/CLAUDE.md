# Catalog Service

Product info, pricing, inventory, and product images.

## Data Owned

- Products, SKUs, Product Images, Stock Levels

## Architecture

- Layered: `routes` → `service` → `domain` → `repository` → DB
- Each module has a `domain.rs` with rich domain model objects where all fields are value objects (not raw primitives)
- `dtos.rs` handles VO validation for requests + FK/cross-entity validation via `Validated*` types
- Modules: `src/products/`, `src/categories/`, `src/brands/`, `src/common/`
- No gRPC sidecar — HTTP only
- Claims-based JWT auth (no user DB lookup)

## File Layout

```
catalog/
├── Cargo.toml
├── CLAUDE.md
├── migrations/
│   └── 202602241106_init.sql      # products, skus, product_images tables
├── src/
│   ├── main.rs                    # run_service_with_infra(), NoGrpc
│   ├── lib.rs                     # AppState { service, jwt_service }, app()
│   ├── common/
│   │   ├── mod.rs
│   │   └── value_objects.rs       # validated_name! macro, Slug, HttpUrl (shared across modules)
│   ├── categories/                # Category CRUD (ltree hierarchy)
│   ├── brands/                    # Brand CRUD + brand-category associations
│   └── products/
│       ├── mod.rs
│       ├── routes.rs              # all HTTP handlers (public + protected)
│       ├── service.rs             # CatalogService — orchestration only
│       ├── domain.rs              # Rich domain models: Product, Sku (all fields are value objects)
│       ├── repository.rs          # SQL queries with LEFT JOINs, FK existence helpers
│       ├── entities.rs            # ProductEntity (raw DB row), SkuEntity, ProductImageEntity
│       ├── dtos.rs                # Request/response DTOs + validated variants (VO + FK validation)
│       └── value_objects.rs       # ProductName, Slug, Price, SkuCode, StockQuantity, Currency, ImageUrl, statuses
└── tests/
    ├── integration.rs             # test entry point
    ├── common/mod.rs              # test_db(), test_app_state(), sample fixtures (seller/buyer/admin users, sample DTOs)
    └── products/
        ├── mod.rs
        ├── repository_test.rs     # repository-level tests
        ├── service_test.rs        # service-level tests
        └── router_test.rs         # HTTP integration tests
```

## Endpoints (`/api/v1/products`)

**Public:**
| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | List active products |
| GET | `/{id}` | Get product detail (with SKUs and images) |
| GET | `/slug/{slug}` | Get product by slug |

**Protected (JWT required):**
| Method | Path | Description |
|--------|------|-------------|
| POST | `/` | Create product (seller) |
| GET | `/seller/me` | List my products |
| PUT | `/{id}` | Update product (owner or admin) |
| DELETE | `/{id}` | Soft delete product (owner or admin) |
| GET | `/{product_id}/skus` | List SKUs for product |
| POST | `/{product_id}/skus` | Create SKU (product owner or admin) |
| PUT | `/skus/{sku_id}` | Update SKU (product owner or admin) |
| DELETE | `/skus/{sku_id}` | Soft delete SKU (product owner or admin) |
| POST | `/skus/{sku_id}/stock` | Adjust stock quantity (`{ "delta": N }`) |
| GET | `/{product_id}/images` | List images |
| POST | `/{product_id}/images` | Add image (product owner or admin) |
| DELETE | `/{product_id}/images/{image_id}` | Delete image (product owner or admin) |

## Domain Models (`domain.rs`)

Rich types where every field is a value object. Business logic goes here.

| Domain Type | Fields (value objects)                                                 | Constructed via              |
|-------------|------------------------------------------------------------------------|------------------------------|
| `Product`   | `ProductName`, `Slug`, `Price`, `Currency`, `ProductStatus` + FK UUIDs | `TryFrom<ProductEntity>`     |
| `Sku`       | `SkuCode`, `Price`, `StockQuantity` + product_id UUID                  | `TryFrom<(Uuid, SkuEntity)>` |
| `Brand`     | `BrandName`, `Slug`, `HttpUrl` (logo)                                  | `TryFrom<BrandEntity>`       |
| `Category`  | `CategoryName`, `Slug`, `LtreeLabel` + parent/depth                    | `TryFrom<CategoryEntity>`    |

FK references are currently `Option<Uuid>` — planned evolution to embedded domain objects for traversable graphs.

## Entities (raw DB rows)

- `ProductEntity` — id, seller_id, name, slug (unique), description, base_price (Decimal), currency, category_id (FK),
  brand_id (FK), status, soft-delete + joined fields: category_name, category_slug, brand_name, brand_slug
- `SkuEntity` — id, product_id, sku_code (unique), price (Decimal), stock_quantity, attributes (JSONB), status,
  soft-delete
- `ProductImageEntity` — id, product_id, url, alt_text, sort_order, is_primary (no soft-delete)

## Value Objects (`src/products/value_objects.rs`)

| Type            | Rules                                                         |
|-----------------|---------------------------------------------------------------|
| `ProductName`   | Non-empty, max 500 chars, trimmed                             |
| `Slug`          | Lowercase alphanumeric with hyphens; auto-generated from name |
| `Price`         | Decimal >= 0                                                  |
| `SkuCode`       | 2-100 chars, alphanumeric with hyphens/underscores            |
| `StockQuantity` | i32 >= 0                                                      |
| `Currency`      | 3-letter ISO 4217 (e.g. USD, KRW), uppercased                 |
| `ImageUrl`      | Must start with http:// or https://, max 2048 chars           |
| `ProductStatus` | Draft, Active, Inactive, Archived                             |
| `SkuStatus`     | Active, Inactive, OutOfStock                                  |

## Key Patterns

- **Money handling:** `rust_decimal::Decimal` + `NUMERIC(19,4)` in Postgres (see ADR-007)
- **Auth:** Claims-based JWT — `AuthMiddleware::new_claims_based()` (see ADR-008)
- **Access control:** `require_access()` — product owner or admin for all mutations
- **Transactions:** All writes use `with_transaction()` from shared
- **Soft deletes:** Products and SKUs use `deleted_at`; images are hard-deleted
- **Partial updates:** Dynamic SQL for product and SKU updates (only provided fields)
- **Domain models:** `domain.rs` has rich types (all fields are VOs); business logic goes here
- **FK validation:** `dtos.rs` validated request types enforce FK existence + brand-category association
- **LEFT JOINs:** All product reads JOIN categories/brands to include names/slugs in responses

## Env Vars

| Var                   | Purpose                                         |
|-----------------------|-------------------------------------------------|
| `CATALOG_DB_URL`      | Postgres connection string                      |
| `CATALOG_PORT`        | HTTP port (default 3000)                        |
| `REDIS_URL`           | Redis connection (optional, for future caching) |
| `ACCESS_TOKEN_SECRET` | JWT access token signing key                    |

## Migrations

Located at `migrations/`, referenced as `./.migrations/catalog` at runtime.

## Tests

58 unit tests (value objects) + 53 integration tests (repository + service + router). Run with:

```
make test SERVICE=catalog
```

### Catalog-specific tips
- Common VOs: `catalog/src/common/value_objects.rs` — `validated_name!` macro, `Slug`, `HttpUrl`
- ltree path column requires `::ltree` cast in raw SQL inserts
- Test fixtures: `create_test_category()`, `create_test_brand()`, `associate_brand_category()` in `tests/common/mod.rs`
