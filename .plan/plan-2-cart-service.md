# Plan 2: Cart Service

## Context

Redis-only microservice for shopping cart storage. First service in the workspace without Postgres. Depends on Plan 1 for `RedisServiceConfig` + `run_redis_service_with_infra()` in shared.

---

## 1. Architecture Decisions

- **Storage**: Redis Hash per user, 30-day TTL, max 50 unique SKUs
- **Auth**: Claims-based JWT (trusts JWT claims, no user DB lookup)
- **No catalog validation on add**: Client provides price snapshot + display data. Validation happens at checkout via `/validate` endpoint (future catalog integration).
- **Price strategy**: Store price at add time. Order service compares against current catalog price at checkout.
- **Cart-to-order**: Cart provides data, Order service creates order and clears cart. Cart is purely a data holder.
- **Bootstrap**: New `run_redis_service_with_infra()` (no Postgres connection)

---

## 2. Endpoints

| Method | Path | Auth | Description | Response |
|--------|------|------|-------------|----------|
| GET | `/api/v1/cart` | Any | Get cart with computed totals | 200 + CartRes |
| POST | `/api/v1/cart/items` | Any | Add/replace item (set semantics) | 200 + CartRes |
| PUT | `/api/v1/cart/items/{sku_id}` | Any | Update quantity only | 200 + CartRes |
| DELETE | `/api/v1/cart/items/{sku_id}` | Any | Remove item (idempotent) | 200 + message |
| DELETE | `/api/v1/cart` | Any | Clear entire cart (idempotent) | 200 + message |
| POST | `/api/v1/cart/validate` | Any | Validate cart (stub) | 200 + ValidationRes |

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
// line_total() -> quantity * unit_price

pub struct Cart {
    pub user_id: Uuid,
    pub items: Vec<CartItem>,
}
// total() -> sum of line_totals
// item_count() -> items.len()
```

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
    total: Decimal,
}
// CartItemRes includes line_total computed at response time

struct CartValidationRes {
    items: Vec<CartValidationItemRes>,
    all_valid: bool,
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
- `validate_cart(user_id)` → CartValidationRes (stub: all valid)

---

## 9. Routes

File: `cart/src/cart/routes.rs`

All routes protected with `AuthMiddleware::new_claims_based(jwt_service)`.
All handlers extract `CurrentUser` and use `current_user.id` as cart owner.
Add/update/get return full cart response for optimistic UI updates.

---

## 10. AppState & Bootstrap

### `cart/src/lib.rs`
```rust
pub struct AppState {
    pub cart_service: Arc<CartService>,
    pub jwt_service: Arc<JwtService>,
}
```

### `cart/src/main.rs`
```rust
run_redis_service_with_infra(
    RedisServiceConfig { name: "cart", port_env_key: "CART_PORT" },
    |redis_conn| {
        let app_state = AppState::new(redis_conn);
        app(app_state).merge(health_routes("cart"))
    },
).await
```

---

## 11. File Structure

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

## 12. Cargo.toml

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

## 13. Error Handling

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

## 14. Env Vars

| Variable | Required | Default | Purpose |
|----------|----------|---------|---------|
| `REDIS_URL` | Yes | — | Redis connection |
| `CART_PORT` | No | 3000 | HTTP port |
| `ACCESS_TOKEN_SECRET` | Yes | — | JWT verification |

---

## 15. Tests (~55 total)

### Value object unit tests (~10)
- Quantity: valid (1-99), reject 0, reject >99
- PriceSnapshot: valid, reject negative
- Currency: valid + normalize, reject wrong length

### Repository tests (~15)
- Set and get cart item roundtrip
- Get empty cart
- Remove item, clear cart
- Item exists true/false
- Cart item count
- TTL is set after write
- Overwrite existing item

### Service tests (~15)
- Add item returns full cart
- Validates quantity, price, product name
- Rejects when cart full (50 items)
- Replaces existing SKU (set semantics)
- Update quantity success + not found
- Remove item idempotent
- Clear cart, get cart computes totals
- Validate cart stub returns all valid

### Router tests (~15)
- GET cart returns 200 (empty)
- GET cart requires auth (401)
- POST add item returns 200 with cart
- POST add item returns 400 (invalid quantity)
- PUT update returns 200 / 404
- DELETE item returns 200
- DELETE cart returns 200
- POST validate returns 200
- Multiple items flow: add, get, remove, verify

---

## 16. Implementation Order

1. Shared: Add `RedisServiceConfig` + `run_redis_service_with_infra` to `server.rs`
2. Workspace: Add `"cart"` to root `Cargo.toml` members
3. `cart/Cargo.toml`
4. Value objects + inline unit tests
5. Domain model (CartItem, Cart)
6. Repository (Redis operations) + repository tests
7. DTOs (request, validated, response) + conversions
8. Service layer + service tests
9. Routes + router tests
10. Bootstrap (lib.rs, main.rs)
11. Module glue (mod.rs, tests/integration.rs, tests/common)
12. `cart/CLAUDE.md`
13. Update root `CLAUDE.md` services table

---

## 17. Changes to Shared Crate

Only one addition to `shared/src/server.rs`:

```rust
pub struct RedisServiceConfig {
    pub name: &'static str,
    pub port_env_key: &'static str,
}

pub async fn run_redis_service_with_infra<F>(
    config: RedisServiceConfig,
    build_app: F,
) -> Result<(), Box<dyn Error>>
where F: FnOnce(redis::aio::ConnectionManager) -> Router
{
    init_tracing(config.name);
    let redis_config = RedisConfig::new();
    let redis_conn = init_redis(redis_config).await;
    let port: u16 = std::env::var(config.port_env_key)
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .expect("PORT must be valid u16");
    let app = build_app(redis_conn);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("{} listening on port {}", config.name, port);
    axum::serve(listener, app).await?;
    Ok(())
}
```
