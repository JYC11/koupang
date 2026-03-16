# Cart Service

Redis-backed shopping cart with DOP checkout validation.

## Architecture

- Redis-only (no Postgres) — hash-per-user (`cart:{user_id}`, field = sku_id, value = JSON)
- DOP rule algebra: `checkout_readiness_rules()` (6 checks) — cart not empty, min/max value, valid prices/quantities
- Claims-based JWT auth (ADR-008)
- No Kafka integration yet (will consume catalog events for price/stock validation)

## File Layout

```
cart/src/
├── main.rs / lib.rs              # AppState { redis, auth_config }
└── cart/                         # domain.rs, dtos.rs, error.rs, repository.rs, routes.rs, rules.rs, service.rs, value_objects.rs
```

Tests: `tests/cart/{repository,service,router}_test.rs` + `tests/common/mod.rs`

## Endpoints (`/api/v1/cart`)

All routes require JWT auth.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Get cart (items, count, total) |
| DELETE | `/` | Clear entire cart |
| POST | `/items` | Add item to cart (max 50 unique SKUs) |
| PUT | `/items/{sku_id}` | Update item quantity |
| DELETE | `/items/{sku_id}` | Remove item from cart |
| POST | `/validate` | Validate cart for checkout readiness |

## Value Objects

| VO | Rules |
|----|-------|
| `Quantity` | u32, 1–99 |
| `PriceSnapshot` | Re-exported `Price` from shared (Decimal >= 0) |
| `Currency` | Re-exported from shared (3-letter ISO 4217) |
| `CartProductName` | Via `validated_name!` macro, max 500 chars |

## Key Patterns

- **30-day TTL** on cart hash, refreshed on every write
- **Max 50 items** enforced at service layer (checks HLEN before adding new SKU)
- **Checkout validation** returns structured response with per-item validation (price_changed, stock_insufficient — stubbed until catalog integration)
- **No Postgres** — uses `TestRedis::start()` with FLUSHDB for test isolation

## Env Vars

`CART_PORT` (default 3000), `REDIS_URL`, `ACCESS_TOKEN_SECRET`

## Tests

28 unit + 31 integration = 59 tests. `make test SERVICE=cart`
