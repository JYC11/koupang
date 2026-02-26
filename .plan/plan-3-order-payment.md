# Plan 3: Order + Payment + Catalog Inventory Extension

## Context

Implement the core purchasing saga: order creation, payment authorization, inventory reservation, and all compensation/rollback flows. This is the largest plan, touching three services.

Depends on Plan 1 (Kafka, outbox, event system).

---

## 1. Order Service

### 1.1 Schema

```sql
CREATE TABLE orders (
    id                UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ,
    buyer_id          UUID NOT NULL,
    status            VARCHAR(50) NOT NULL DEFAULT 'pending',
    total_amount      NUMERIC(19, 4) NOT NULL,
    currency          VARCHAR(3) NOT NULL DEFAULT 'USD',
    idempotency_key   VARCHAR(255) NOT NULL UNIQUE,
    shipping_address  JSONB NOT NULL DEFAULT '{}',
    cancelled_reason  TEXT,
    CONSTRAINT chk_orders_total CHECK (total_amount >= 0),
    CONSTRAINT chk_orders_status CHECK (status IN (
        'pending', 'inventory_reserved', 'payment_authorized',
        'confirmed', 'shipped', 'delivered', 'cancelled', 'returned'
    ))
);

CREATE TABLE order_items (
    id            UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    order_id      UUID NOT NULL REFERENCES orders (id),
    product_id    UUID NOT NULL,
    sku_id        UUID NOT NULL,
    product_name  VARCHAR(500) NOT NULL,
    sku_code      VARCHAR(100) NOT NULL,
    quantity      INTEGER NOT NULL,
    unit_price    NUMERIC(19, 4) NOT NULL,
    total_price   NUMERIC(19, 4) NOT NULL,
    CONSTRAINT chk_quantity CHECK (quantity > 0),
    CONSTRAINT chk_unit_price CHECK (unit_price >= 0),
    CONSTRAINT chk_total_price CHECK (total_price >= 0)
);

CREATE TABLE outbox (...);           -- same as Plan 1 template
CREATE TABLE processed_events (...); -- same as Plan 1 template
```

Key decisions:
- `shipping_address` is JSONB (snapshot at order time)
- `product_name` + `sku_code` snapshotted (order is self-contained)
- `idempotency_key` is UNIQUE (enforces at-most-once creation)

### Comment on shipping
- We haven't planned shipping yet but this area I think would be also another very complex area
- Some thinking into what the requirements could be for shipping could help shape the design
- we may need to consider how to handle shipping costs, etc and how it contributes to total cost (which affects cart logic)
- we may also need to consider receiving shipping address information from the buyer (affects identity service)

### Comment on idempotency
- Idempotency key should also be used for payment processing

### 1.2 State Machine

```
Pending → InventoryReserved → PaymentAuthorized → Confirmed → Shipped → Delivered → Returned
   ↓              ↓                    ↓               ↓
Cancelled    Cancelled           Cancelled        Cancelled
```

```rust
pub enum OrderStatus {
    Pending, InventoryReserved, PaymentAuthorized,
    Confirmed, Shipped, Delivered, Cancelled, Returned,
}

impl OrderStatus {
    pub fn transition_to(&self, target: &OrderStatus) -> Result<OrderStatus, AppError> {
        // Validates transition is in the allowed set
    }
}
```

### 1.3 Value Objects

- `OrderId`, `OrderItemId` (via `shared::valid_id!`)
- `OrderStatus` (enum with transition validation)
- `ShippingAddress` (JSONB wrapper: street, city, state, postal_code, country)
- `Quantity` (i32 > 0)
- `IdempotencyKey` (non-empty, max 255)
- `Price`, `Currency` (defined locally, same pattern as catalog)

### 1.4 Endpoints

| Method | Path | Auth | Response |
|--------|------|------|----------|
| POST | `/api/v1/orders` | Buyer | 202 Accepted (requires Idempotency-Key header) |
| GET | `/api/v1/orders/{id}` | Owner/Admin | Order detail + items |
| GET | `/api/v1/orders` | Owner/Admin | Paginated list (keyset) |
| POST | `/api/v1/orders/{id}/cancel` | Owner/Admin | Cancel if status allows |

### 1.5 Create Order Flow

1. Extract `Idempotency-Key` header (required, 400 if missing)
2. Check if order with this key exists → return existing (idempotent)
3. Validate DTOs: items non-empty, quantities > 0, compute totals
4. `with_transaction`:
   - Insert order (status=Pending)
   - Insert order items
   - Insert `OrderCreated` into outbox
5. Return 202 Accepted

### 1.6 Kafka Consumers

Spawned as background tasks in `main.rs`:

- **inventory.events consumer** (group: `order-service`):
  - `InventoryReserved` → update status to InventoryReserved
  - `InventoryReservationFailed` → cancel order

- **payments.events consumer** (group: `order-service`):
  - `PaymentAuthorized` → if status == InventoryReserved, transition to Confirmed, write `OrderConfirmed` to outbox
  - `PaymentFailed` → cancel order, write `OrderCancelled` to outbox

All handlers check `processed_events` for idempotency.

### 1.7 File Structure

```
order/src/
├── main.rs, lib.rs
├── orders/        routes, service, domain, entities, dtos, value_objects, repository
├── outbox/        entities, repository, relay
├── consumers/     inventory_events, payment_events
└── events/        types (OrderCreated, OrderConfirmed, OrderCancelled payloads)
```

---

## 2. Payment Service

### 2.1 Schema

```sql
CREATE TABLE payments (
    id                UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ,
    order_id          UUID NOT NULL,
    buyer_id          UUID NOT NULL,
    amount            NUMERIC(19, 4) NOT NULL,
    currency          VARCHAR(3) NOT NULL DEFAULT 'USD',
    status            VARCHAR(50) NOT NULL DEFAULT 'pending',
    gateway_reference VARCHAR(255),
    failure_reason    TEXT,
    CONSTRAINT chk_amount CHECK (amount > 0),
    CONSTRAINT chk_status CHECK (status IN (
        'pending', 'authorized', 'captured', 'failed', 'refunded', 'voided'
    ))
);

CREATE TABLE outbox (...);
CREATE TABLE processed_events (...);
```

### 2.2 Payment Status State Machine

```
Pending → Authorized → Captured
  ↓           ↓
Failed      Voided → (if captured) Refunded
```
## Comment on Payments
- when building payments, we have to think of the future of the system where refunds, subscription, and disbursements(give money to sellers) are possible.
- so the information has to be stored in an event store manner for flexibility of the data
- the state machine part is technically correct but the nature of data is that it's more of an event store
- consider accounting concepts like ledger, ledger entries, credit/debit etc when building the financial part of the system
- we may also need to consider saving payment information for future use (could affect identity service, where to store payment information???)
- also standard payment gateway timeout/retry mechanisms to prevent deadlock in one state
- also, how to handle out of order events? need some kinda state machine validation
  - eg, payment can timeout and "fail" and then later suddenly succeed, so we need to handle that

### 2.3 Mock Payment Gateway

```rust
#[async_trait]
pub trait PaymentGateway: Send + Sync {
    async fn authorize(&self, order_id: Uuid, amount: Decimal, currency: &str)
        -> Result<GatewayAuthResult, AppError>;
    async fn capture(&self, gateway_reference: &str) -> Result<GatewayCaptureResult, AppError>;
    async fn void(&self, gateway_reference: &str) -> Result<GatewayVoidResult, AppError>;
    async fn refund(&self, gateway_reference: &str, amount: Decimal)
        -> Result<GatewayRefundResult, AppError>;
}

pub struct MockPaymentGateway { success_rate: f64 }
// ::always_succeeds() / ::always_fails() for testing
// Simulated 50ms latency, random reference IDs
```

Follows ADR-006 pattern (EmailService trait with mock).

### 2.4 Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/payments/{order_id}` | Owner/Admin | Payment status for order |

Payment service is primarily event-driven. Most logic lives in consumers.

## Comment on order endpoints
- we need minimal endpoints for sellers and buyers to see their orders

### 2.5 Kafka Consumers

- **inventory.events** (group: `payment-service`):
  - `InventoryReserved` → authorize payment via gateway → write `PaymentAuthorized` or `PaymentFailed` to outbox

- **orders.events** (group: `payment-service`):
  - `OrderConfirmed` → capture authorized payment
  - `OrderCancelled` → void (if authorized) or refund (if captured)

### 2.6 File Structure

```
payment/src/
├── main.rs, lib.rs
├── payments/      routes, service, domain, entities, dtos, value_objects, repository
├── gateway/       traits (PaymentGateway), mock (MockPaymentGateway)
├── outbox/        entities, repository, relay
├── consumers/     inventory_events, order_events
└── events/        types (PaymentAuthorized, PaymentFailed, PaymentCaptured, PaymentVoided)
```

---

## 3. Catalog Inventory Extension

### 3.1 New Migration

```sql
ALTER TABLE skus ADD COLUMN reserved_quantity INTEGER NOT NULL DEFAULT 0;
ALTER TABLE skus ADD CONSTRAINT chk_reserved CHECK (reserved_quantity >= 0);
ALTER TABLE skus ADD CONSTRAINT chk_available CHECK (stock_quantity >= reserved_quantity);

CREATE TABLE inventory_reservations (
    id          UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    order_id    UUID NOT NULL,
    sku_id      UUID NOT NULL REFERENCES skus (id),
    quantity    INTEGER NOT NULL,
    status      VARCHAR(50) NOT NULL DEFAULT 'reserved',
    released_at TIMESTAMPTZ,
    CONSTRAINT chk_quantity CHECK (quantity > 0),
    CONSTRAINT chk_status CHECK (status IN ('reserved', 'confirmed', 'released'))
);
CREATE UNIQUE INDEX idx_reservations_order_sku
    ON inventory_reservations (order_id, sku_id) WHERE status = 'reserved';

CREATE TABLE outbox (...);
CREATE TABLE processed_events (...);
```

Available stock = `stock_quantity - reserved_quantity`.
## Comment on Inventory Reservations
- This will effect showing inventory in the UI for read endpoints
- Consider how we can accomplish this, perhaps a materialized view/regular view? Or some other option
- Inventory reservation could fail
- For "at the same time" reservations, we may need to think of some mitigation strategies like locking or queuing or some other mechanism

Separate `inventory_reservations` table:
- Tracks which order owns each reservation
- Enables targeted release (only release order X's inventory)
- Provides audit trail

### 3.2 Reservation Flow

1. `OrderCreated` event received
2. For each item: `SELECT sku FOR UPDATE` → check available → increment reserved → create reservation
3. All in single transaction
4. Success → write `InventoryReserved` to outbox
5. Failure (any item insufficient) → write `InventoryReservationFailed` to outbox

### 3.3 Release Flow (on `OrderCancelled`)

1. Get all reservations for order
2. For each: decrement `reserved_quantity`, set reservation status=released
3. All in single transaction

### 3.4 Confirm Flow (on `OrderConfirmed`)

1. Get all reservations for order
2. For each: `stock_quantity -= N`, `reserved_quantity -= N`, set reservation status=confirmed
3. All in single transaction (actual stock deduction)

### 3.5 New Module: `catalog/src/inventory/`

```
catalog/src/inventory/
├── mod.rs, service.rs, repository.rs
├── entities.rs, dtos.rs, value_objects.rs
catalog/src/outbox/       # New for catalog
catalog/src/consumers/    # order_events.rs
```

### 3.6 Catalog Changes

- `AppState` gains `inventory_service: Arc<InventoryService>`
- `app()` merges inventory routes
- `Cargo.toml` adds `shared = { features = ["kafka"] }`
- New Kafka consumer spawned in `main.rs`

---

## 4. Domain Events

### OrderCreated (topic: `orders.events`)
```json
{
  "order_id": "uuid", "buyer_id": "uuid",
  "total_amount": "49.98", "currency": "USD",
  "items": [{ "product_id": "uuid", "sku_id": "uuid", "quantity": 2, "unit_price": "24.99" }]
}
```

### InventoryReserved (topic: `inventory.events`)
```json
{
  "order_id": "uuid", "buyer_id": "uuid",
  "total_amount": "49.98", "currency": "USD",
  "items": [{ "sku_id": "uuid", "quantity": 2 }]
}
```

### InventoryReservationFailed (topic: `inventory.events`)
```json
{ "order_id": "uuid", "reason": "Insufficient stock for SKU xyz" }
```

### PaymentAuthorized (topic: `payments.events`)
```json
{ "order_id": "uuid", "payment_id": "uuid", "gateway_reference": "mock-auth-xyz" }
```

### PaymentFailed (topic: `payments.events`)
```json
{ "order_id": "uuid", "reason": "Payment declined" }
```

### OrderConfirmed (topic: `orders.events`)
```json
{ "order_id": "uuid", "buyer_id": "uuid" }
```

### OrderCancelled (topic: `orders.events`)
```json
{ "order_id": "uuid", "buyer_id": "uuid", "reason": "Payment failed" }
```

---

## 5. Complete Saga Flows

### Happy Path
```
POST /orders → OrderCreated
  → [Catalog] reserve inventory → InventoryReserved
  → [Payment] authorize → PaymentAuthorized
  → [Order] status=Confirmed → OrderConfirmed
  → [Catalog] deduct stock + [Payment] capture
```

### Failure: Inventory Unavailable
```
OrderCreated → [Catalog] insufficient stock → InventoryReservationFailed
  → [Order] cancel (no compensation needed)
```

### Failure: Payment Declined
```
OrderCreated → InventoryReserved → [Payment] declined → PaymentFailed
  → [Order] cancel → OrderCancelled → [Catalog] release inventory
```

### Manual Cancellation (from buyer side)
```
POST /orders/{id}/cancel → OrderCancelled
  → [Catalog] release inventory + [Payment] void/refund
```

## Comment on Saga Flows
- More failure cases:
  - Seller refuses to fulfill order for whatever reason besides inventory
  - Timeout cases where payment is in "PENDING" forever

---

## 6. Compensation Summary

| Trigger | Reactor | Action |
|---------|---------|--------|
| InventoryReservationFailed | Order | Cancel order |
| PaymentFailed | Order | Cancel order → OrderCancelled |
| OrderCancelled (inv reserved) | Catalog | Release reserved stock |
| OrderCancelled (pay authorized) | Payment | Void payment |
| OrderCancelled (pay captured) | Payment | Refund payment |

---

## 7. Testing

## Comment on testing
- revisit test cases after referring to test-standards.md
- also think of how to test the entire saga flow with all the services up
- how should we test various infra failure cases? we can try simulating them somehow

### Order Service (~80 tests)
- Value objects: status transitions, idempotency key, quantity (~15)
- Repository: CRUD, outbox, processed_events (~20)
- Service: create, handle events, idempotency, cancel (~25)
- Router: HTTP integration (~20)

### Payment Service (~60 tests)
- Value objects: payment status transitions (~10)
- Gateway: mock authorize/capture/void/refund (~10)
- Repository: payment CRUD, outbox (~15)
- Service: event handlers with success/fail mocks (~15)
- Router: HTTP integration (~10)

### Catalog Inventory (~40 tests)
- Reserve stock, release, confirm (~15)
- Edge cases: insufficient stock, duplicate reservation (~10)
- Event handler integration (~15)

### Saga Flow Tests (~10)
Direct service-to-service calls (no Kafka) simulating complete flows:
```rust
async fn saga_happy_path() {
    // 1. Create order → read outbox
    // 2. Call inventory_service.handle_order_created() → read outbox
    // 3. Call payment_service.handle_inventory_reserved() → read outbox
    // 4. Call order_service.handle_payment_authorized() → verify confirmed
}
```

### Other Comments:
- how to prevent cascading failures?
- how to ensure data drift/inconsistencies does not occur? particularly with money related stuff

---

## 8. Implementation Order

1. Order: schema + repository + entities
2. Order: value objects + domain + DTOs
3. Order: service (create, get, list, cancel — no events yet)
4. Order: routes + router tests
5. Order: outbox + relay
6. Payment: schema + repository
7. Payment: gateway trait + mock
8. Payment: service + event handlers
9. Payment: routes
10. Catalog: inventory migration + repository
11. Catalog: inventory service + event handlers
12. Wire Kafka consumers in each main.rs
13. Saga integration tests
14. CLAUDE.md for order and payment

---

## 9. Cargo.toml (Order)

```toml
[dependencies]
axum = "0.8.8"
shared = { path = "../shared", features = ["kafka"] }
tokio = { version = "1.49.0", features = ["full"] }
sqlx = { version = "0.8.6", features = ["runtime-tokio", "postgres", "uuid", "chrono", "rust_decimal"] }
uuid = { version = "1.21.0", features = ["v4", "v7", "serde"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
chrono = { version = "0.4.43", features = ["serde"] }
rust_decimal = { version = "1.4.0", features = ["serde"] }
redis = { version = "1.0.4", features = ["tokio-comp", "connection-manager"] }
rdkafka = { version = "0.37", features = ["cmake-build"] }
tower = "0.5.3"
tower-http = { version = "0.6.8", features = ["trace"] }
tracing = "0.1.44"
async-trait = "0.1"

[dev-dependencies]
shared = { path = "../shared", features = ["test-utils", "kafka"] }
```

Payment `Cargo.toml` is similar, plus `rand` for mock gateway.
