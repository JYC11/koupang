# Plan 2: Cart Service (Revised)

## Context

Redis-only microservice for shopping cart storage. First service in the workspace without Postgres. Depends on Plan 1 for `ServiceBuilder` with `.with_redis()`.

---

## 1. Architecture Decisions

- **Storage**: Redis Hash per user, 30-day TTL, max 50 unique SKUs
- **Auth**: Claims-based JWT (trusts JWT claims, no user DB lookup)
- **No catalog validation on add**: Client provides price snapshot + display data
- **Cart totals are display-only**: `line_total = qty * unit_price`, `cart_total = sum(line_totals)`. No tax, shipping, or discounts. These are UI estimates only.
- **Order service owns authoritative totals**: At checkout, Order re-fetches current prices from Catalog and computes the real total. Cart snapshots are for display convenience.
- **Price drift / staleness**: Caught at validate or checkout time (see `/validate` endpoint). Cart holds snapshots and doesn't care about catalog changes — expired items naturally clean up via 30-day TTL.
- **No shared pricing crate yet**: Premature. Extract later if discounts/coupons (P4) require shared logic.
- **Cart-to-order**: Cart provides data, Order service creates order and clears cart. Cart is purely a data holder.
- **Bootstrap**: Via `ServiceBuilder::new("cart").with_redis()` (Plan 1)

---

## 2. Endpoints

| Method | Path | Auth | Description | Response |
|--------|------|------|-------------|----------|
| GET | `/api/v1/cart` | Any | Get cart with computed totals | 200 + CartRes |
| POST | `/api/v1/cart/items` | Any | Add/replace item (set semantics) | 200 + CartRes |
| PUT | `/api/v1/cart/items/{sku_id}` | Any | Update quantity only | 200 + CartRes |
| DELETE | `/api/v1/cart/items/{sku_id}` | Any | Remove item (idempotent) | 200 + message |
| DELETE | `/api/v1/cart` | Any | Clear entire cart (idempotent) | 200 + message |
| POST | `/api/v1/cart/validate` | Any | Validate cart against Catalog | 200 + ValidationRes |

---

## 3. Redis Data Model

### Key pattern
```
cart:{user_id}    # Redis Hash
```

### Hash fields
- Field key: `{sku_id}` (UUID string)
- Field value: JSON string

```json
{
  "product_id": "uuid",
  "sku_id": "uuid",
  "quantity": 2,
  "unit_price": "24.99",
  "currency": "USD",
  "product_name": "Widget Pro",
  "image_url": "https://cdn.example.com/img.jpg",
  "added_at": "2026-02-25T10:30:00Z"
}
```

### TTL
- 30 days (2,592,000 seconds)
- Refreshed on **writes** only (add, update, remove)
- Reads do NOT refresh TTL

### Max items
- 50 unique SKUs per cart
- Enforced via `HLEN` check before adding new item
- Updating existing item does not count against limit

---

## 4. Value Objects

File: `cart/src/cart/value_objects.rs`

| VO | Validation | Inner Type |
|----|-----------|------------|
| `Quantity` | 1-99 | u32 |
| `PriceSnapshot` | >= 0 | Decimal |
| `Currency` | 3-letter ISO 4217, uppercased, default USD | String |
| `CartProductName` | via `shared::validated_name!` macro, max 500 | String |

---

## 5. Domain Model

File: `cart/src/cart/domain.rs`

```rust
pub struct CartItem {
    pub product_id: Uuid,
    pub sku_id: Uuid,
    pub quantity: Quantity,
    pub unit_price: PriceSnapshot,
    pub currency: Currency,
    pub product_name: String,
    pub image_url: Option<String>,
    pub added_at: DateTime<Utc>,
}
// line_total() -> quantity * unit_price (display-only estimate)

pub struct Cart {
    pub user_id: Uuid,
    pub items: Vec<CartItem>,
}
// total() -> sum of line_totals (display-only estimate)
// item_count() -> items.len()
```

Cart math is intentionally simple — no tax, shipping, or discounts. Order service computes authoritative totals at checkout.

---

## 6. DTOs

File: `cart/src/cart/dtos.rs`

### Request
```rust
struct AddToCartReq {
    product_id: Uuid,
    sku_id: Uuid,
    quantity: u32,
    unit_price: Decimal,
    currency: Option<String>,    // defaults to USD
    product_name: String,
    image_url: Option<String>,
}

struct UpdateCartItemReq {
    quantity: u32,
}
```

### Validated (via TryFrom)
```rust
struct ValidAddToCartReq { /* all fields as value objects */ }
struct ValidUpdateCartItemReq { quantity: Quantity }
```

### Response
```rust
struct CartRes {
    items: Vec<CartItemRes>,
    item_count: usize,
    total: Decimal,          // display-only estimate
}
// CartItemRes includes line_total computed at response time

struct CartValidationRes {
    items: Vec<CartValidationItemRes>,
    all_valid: bool,
}

struct CartValidationItemRes {
    sku_id: Uuid,
    price_changed: bool,
    snapshot_price: Decimal,
    current_price: Option<Decimal>,   // None if product unavailable
    product_unavailable: bool,        // deleted or inactive in catalog
    stock_insufficient: bool,         // quantity > available stock
}
```

---

## 7. Repository (Redis Operations)

File: `cart/src/cart/repository.rs`

Intermediate type: `CartItemStored` (serde struct for Redis JSON values)

| Function | Redis Commands | Notes |
|----------|---------------|-------|
| `get_cart(conn, user_id)` | `HGETALL` | Deserialize all fields to Vec |
| `get_cart_item(conn, user_id, sku_id)` | `HGET` | Single item lookup |
| `cart_item_count(conn, user_id)` | `HLEN` | For max-items check |
| `set_cart_item(conn, user_id, sku_id, stored)` | `HSET` + `EXPIRE` | Add/update + refresh TTL |
| `remove_cart_item(conn, user_id, sku_id)` | `HDEL` + `EXPIRE` | Remove + refresh TTL |
| `clear_cart(conn, user_id)` | `DEL` | Remove entire hash |
| `item_exists(conn, user_id, sku_id)` | `HEXISTS` | Boolean check |

All functions take `&mut redis::aio::ConnectionManager` and return `Result<_, AppError>`.

---

## 8. Service Layer

File: `cart/src/cart/service.rs`

```rust
pub struct CartService {
    redis: redis::aio::ConnectionManager,
}
```

Methods:
- `get_cart(user_id)` → Cart
- `add_item(user_id, AddToCartReq)` → Cart (validates, checks max items, sets item, returns full cart)
- `update_item_quantity(user_id, sku_id, UpdateCartItemReq)` → Cart (404 if not found)
- `remove_item(user_id, sku_id)` → () (idempotent)
- `clear_cart(user_id)` → () (idempotent)
- `validate_cart(user_id)` → CartValidationRes (checks each item against Catalog: price drift, availability, stock)

---

## 9. Validate Endpoint

`POST /api/v1/cart/validate` — pre-checkout validation against Catalog:

For each cart item, checks:
- **Price drift**: `snapshot_price` vs current catalog price → `price_changed: bool`
- **Product availability**: product deleted or inactive → `product_unavailable: bool`
- **Stock sufficiency**: quantity > available stock → `stock_insufficient: bool`

Returns `CartValidationRes` with `all_valid: bool` summary. Client can prompt user on changes before proceeding to checkout.

**Note**: Initial implementation may stub the Catalog call (returns all valid) until inter-service communication is established. The DTO shape and endpoint are ready for real integration.

---

## 10. Routes

File: `cart/src/cart/routes.rs`

All routes protected with `AuthMiddleware::new_claims_based(jwt_service)`.
All handlers extract `CurrentUser` and use `current_user.id` as cart owner.
Add/update/get return full cart response for optimistic UI updates.

---

## 11. AppState & Bootstrap

### `cart/src/lib.rs`
```rust
pub struct AppState {
    pub cart_service: Arc<CartService>,
    pub jwt_service: Arc<JwtService>,
}
```

### `cart/src/main.rs`
```rust
ServiceBuilder::new("cart")
    .with_redis()
    .build(|infra| {
        let app_state = AppState::new(infra.redis_conn());
        app(app_state).merge(health_routes("cart"))
    })
    .run()
    .await
```

---

## 12. File Structure

```
cart/
├── Cargo.toml
├── CLAUDE.md
├── src/
│   ├── main.rs
│   ├── lib.rs
│   └── cart/
│       ├── mod.rs
│       ├── routes.rs
│       ├── service.rs
│       ├── domain.rs
│       ├── dtos.rs
│       ├── repository.rs
│       └── value_objects.rs
└── tests/
    ├── integration.rs
    ├── common/mod.rs
    └── cart/
        ├── mod.rs
        ├── repository_test.rs
        ├── service_test.rs
        └── router_test.rs
```

---

## 13. Cargo.toml

```toml
[package]
name = "cart"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8.8"
shared = { path = "../shared" }
tokio = { version = "1.49.0", features = ["full"] }
uuid = { version = "1.21.0", features = ["v4", "v7", "serde"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
chrono = { version = "0.4.43", features = ["serde"] }
rust_decimal = { version = "1.4.0", features = ["serde"] }
redis = { version = "1.0.4", features = ["tokio-comp", "connection-manager"] }
tower = "0.5.3"
tower-http = { version = "0.6.8", features = ["trace"] }
tracing = "0.1.44"

[dev-dependencies]
shared = { path = "../shared", features = ["test-utils"] }
```

**Notable: No `sqlx` dependency.** First service without SQL.

---

## 14. Error Handling

| Scenario | AppError Variant | Status |
|----------|-----------------|--------|
| Invalid quantity (0 or >99) | BadRequest | 400 |
| Negative price | BadRequest | 400 |
| Empty product name | BadRequest | 400 |
| Cart full (>50 items) | BadRequest | 400 |
| Item not found (update) | NotFound | 404 |
| Missing auth | (middleware) | 401 |
| Redis failure | InternalServerError | 500 |

---

## 15. Env Vars

| Variable | Required | Default | Purpose |
|----------|----------|---------|---------|
| `REDIS_URL` | Yes | — | Redis connection |
| `CART_PORT` | No | 3000 | HTTP port |
| `ACCESS_TOKEN_SECRET` | Yes | — | JWT verification |

---

## 16. Tests (~45 total, per test-standards.md)

### Value object unit tests (~10)
- Quantity: valid (1-99), reject 0, reject >99
- PriceSnapshot: valid, reject negative
- Currency: valid + normalize, reject wrong length

### Repository tests (~12) — Redis testcontainer, data mechanics only
- Set and get cart item roundtrip
- Get empty cart
- Remove item, clear cart
- Item exists true/false
- Cart item count
- TTL is set after write
- Overwrite existing item

### Service tests (~12) — real Redis, business rules focus
- Add item returns full cart
- Rejects when cart full (50 items)
- Replaces existing SKU (set semantics)
- Update quantity success + not found
- Remove item idempotent
- Clear cart, get cart computes totals
- Validate cart returns expected structure
(Validation errors like bad quantity/price are covered by value object unit tests, not duplicated here)

### Router tests (~11) — full HTTP integration, shape + auth focus
- GET cart returns 200 (empty)
- GET cart requires auth (401)
- POST add item returns 200 with cart
- POST add item returns 400 (invalid quantity — verifies HTTP error shape)
- PUT update returns 200 / 404
- DELETE item returns 200
- DELETE cart returns 200
- POST validate returns 200
- Multiple items flow: add, get, remove, verify

---

## 17. Implementation Order

1. Workspace: Add `"cart"` to root `Cargo.toml` members
2. `cart/Cargo.toml`
3. Value objects + inline unit tests
4. Domain model (CartItem, Cart)
5. Repository (Redis operations) + repository tests
6. DTOs (request, validated, response) + conversions
7. Service layer + service tests
8. Routes + router tests
9. Bootstrap (lib.rs, main.rs) — uses `ServiceBuilder` from Plan 1
10. Module glue (mod.rs, tests/integration.rs, tests/common)
11. `cart/CLAUDE.md`
12. Update root `CLAUDE.md` services table
