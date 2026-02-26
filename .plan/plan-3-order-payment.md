# Plan 3: Order + Payment + Catalog Inventory Extension (Revised)

## Context

Implement the core purchasing saga: order creation, payment authorization, inventory reservation, and all compensation/rollback flows. This is the largest plan, touching three services.

Depends on Plan 1 (Kafka, outbox-core, event system, ServiceBuilder).

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

CREATE TABLE outbox (...);           -- per outbox-core requirements
CREATE TABLE processed_events (...); -- same as Plan 1 template
```

Key decisions:
- `shipping_address` is JSONB (snapshot at order time). Buyer provides it at checkout via `POST /orders` request body — not stored in identity service. Users can ship to different addresses per order.
- `product_name` + `sku_code` snapshotted (order is self-contained)
- `idempotency_key` is UNIQUE (enforces at-most-once creation)
- `total_amount` covers item costs only. Shipping costs will be added as a separate column when shipping service is implemented (no breaking change needed).

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
| GET | `/api/v1/orders` | Buyer | My orders (paginated, keyset) |
| GET | `/api/v1/orders/seller/me` | Seller | Orders containing my products (paginated, keyset) |
| POST | `/api/v1/orders/{id}/cancel` | Owner/Admin | Cancel if status allows |

The seller endpoint filters orders that contain at least one item where `product.seller_id` matches the current user. Mirrors catalog pattern (`GET /products/seller/me`).

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
  - `PaymentTimedOut` → cancel order, write `OrderCancelled` to outbox

All handlers check `processed_events` for idempotency.

### 1.7 File Structure

```
order/src/
├── main.rs, lib.rs
├── orders/        routes, service, domain, entities, dtos, value_objects, repository
├── outbox/        entities, repository, relay (via outbox-core)
├── consumers/     inventory_events, payment_events
└── events/        types (OrderCreated, OrderConfirmed, OrderCancelled payloads)
```

---

## 2. Payment Service — Double-Entry Ledger

### 2.1 Why Double-Entry

Inspired by [Engineers Do Not Get To Make Startup Mistakes When They Build Ledgers](https://news.alvaroduran.com/p/engineers-do-not-get-to-make-startup) and real-world experience from companies like Uber, Square, and Airbnb who all eventually rebuilt their payment systems around double-entry accounting.

**Single-entry problems we're avoiding:**
- Tracks THAT money moved but not WHERE between — cannot answer "how much do we owe sellers?"
- Cannot represent time/promises (authorized != captured)
- Rollbacks and partial failures become undebuggable
- Impossibly difficult to retrofit later

**Double-entry gives us:**
- Every dollar is always accounted for (system balances to zero)
- Natural support for refunds, partial refunds, disbursements, platform commission
- Append-only entries = full audit trail, no data loss
- Payment state derived from entries, not a mutable status column

### 2.2 Three-Entity Schema (Accounts, Transactions, Entries)

```sql
-- Accounts: buckets of value representing a point of view
CREATE TABLE accounts (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    account_type    VARCHAR(50) NOT NULL,
    normal_balance  VARCHAR(10) NOT NULL,   -- 'debit' or 'credit'
    reference_id    UUID NOT NULL,          -- order_id, seller_id, or platform singleton
    currency        VARCHAR(3) NOT NULL DEFAULT 'USD',
    CONSTRAINT chk_normal_balance CHECK (normal_balance IN ('debit', 'credit')),
    CONSTRAINT chk_account_type CHECK (account_type IN (
        'buyer', 'gateway_holding', 'platform_revenue', 'seller_payable'
    ))
);
CREATE INDEX idx_accounts_ref ON accounts (reference_id, account_type);

-- Transactions: group paired entries, handle partial failures
CREATE TABLE ledger_transactions (
    id                UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    order_id          UUID NOT NULL,
    transaction_type  VARCHAR(50) NOT NULL,
    status            VARCHAR(20) NOT NULL DEFAULT 'pending',
    idempotency_key   VARCHAR(255) NOT NULL UNIQUE,
    gateway_reference VARCHAR(255),
    metadata          JSONB NOT NULL DEFAULT '{}',
    CONSTRAINT chk_transaction_type CHECK (transaction_type IN (
        'authorization', 'capture', 'void', 'refund'
    )),
    CONSTRAINT chk_status CHECK (status IN ('pending', 'posted', 'discarded'))
);

-- Entries: always in pairs (debit one account, credit another)
CREATE TABLE ledger_entries (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    transaction_id  UUID NOT NULL REFERENCES ledger_transactions (id),
    account_id      UUID NOT NULL REFERENCES accounts (id),
    direction       VARCHAR(10) NOT NULL,
    amount          NUMERIC(19, 4) NOT NULL,
    status          VARCHAR(20) NOT NULL DEFAULT 'pending',
    CONSTRAINT chk_amount CHECK (amount > 0),
    CONSTRAINT chk_direction CHECK (direction IN ('debit', 'credit')),
    CONSTRAINT chk_status CHECK (status IN ('pending', 'posted', 'discarded'))
);

-- Convenience: derive current payment state per order
CREATE VIEW payment_status AS
SELECT DISTINCT ON (order_id)
    order_id,
    transaction_type,
    status AS transaction_status,
    CASE
        WHEN transaction_type = 'authorization' AND status = 'posted' THEN 'authorized'
        WHEN transaction_type = 'capture' AND status = 'posted' THEN 'captured'
        WHEN transaction_type = 'void' AND status = 'posted' THEN 'voided'
        WHEN transaction_type = 'refund' AND status = 'posted' THEN 'refunded'
        WHEN status = 'pending' THEN 'pending'
        WHEN status = 'discarded' THEN 'failed'
    END AS payment_state
FROM ledger_transactions
ORDER BY order_id, created_at DESC;

-- Convenience: account balances
CREATE VIEW account_balances AS
SELECT
    a.id AS account_id,
    a.account_type,
    a.reference_id,
    a.normal_balance,
    a.currency,
    COALESCE(SUM(CASE WHEN e.direction = 'debit' THEN e.amount ELSE 0 END), 0) AS total_debits,
    COALESCE(SUM(CASE WHEN e.direction = 'credit' THEN e.amount ELSE 0 END), 0) AS total_credits,
    CASE a.normal_balance
        WHEN 'debit' THEN COALESCE(SUM(CASE WHEN e.direction = 'debit' THEN e.amount ELSE -e.amount END), 0)
        WHEN 'credit' THEN COALESCE(SUM(CASE WHEN e.direction = 'credit' THEN e.amount ELSE -e.amount END), 0)
    END AS balance
FROM accounts a
LEFT JOIN ledger_entries e ON e.account_id = a.id AND e.status = 'posted'
GROUP BY a.id, a.account_type, a.reference_id, a.normal_balance, a.currency;

CREATE TABLE outbox (...);
CREATE TABLE processed_events (...);
```

### 2.3 Account Types

| Account | Type | Normal Balance | Represents |
|---------|------|---------------|------------|
| `buyer:{order_id}` | Asset | Debit | Money buyer committed to this order |
| `gateway_holding:{order_id}` | Asset | Debit | Money held at payment gateway |
| `platform_revenue:{order_id}` | Revenue | Credit | Platform's earned revenue |
| `seller_payable:{order_id}` | Liability | Credit | Amount owed to seller |

Accounts are created per-order at authorization time. For global reporting (e.g., "total owed to all sellers"), aggregate across all `seller_payable` accounts.

### 2.4 Money Flows as Paired Entries

**Authorization** (buyer commits money → gateway holds it):
```
Transaction: authorization (pending → posted on gateway success)
  Debit:  gateway_holding:{order_id}  +$49.98
  Credit: buyer:{order_id}            +$49.98
```

**Capture** (gateway holding → platform + seller):
```
Transaction: capture (posted)
  Debit:  platform_revenue:{order_id}  +$49.98   ← full amount for now
  Credit: gateway_holding:{order_id}   +$49.98
```

**Void** (undo authorization, discard original entries):
```
Original authorization transaction → status = discarded
New transaction: void (posted)
  Debit:  buyer:{order_id}            +$49.98   ← money back to buyer
  Credit: gateway_holding:{order_id}  +$49.98
```

**Refund** (reverse capture):
```
Transaction: refund (posted)
  Debit:  gateway_holding:{order_id}   +$49.98  ← money back through gateway
  Credit: platform_revenue:{order_id}  +$49.98  ← reverse the revenue
```

Every transaction always balances: `sum(debits) == sum(credits)`.

### 2.5 Entry Statuses (Saga-Friendly)

Per the article's recommendation:
- **pending**: Entry created, waiting for external confirmation (e.g., gateway response)
- **posted**: Confirmed, affects account balances
- **discarded**: Failed or voided, does NOT affect balances but preserved for audit

Entries are never deleted. Failed authorizations are discarded (not reversed), keeping history clean while preserving auditability.

### 2.6 Platform Commission (Future Work — Out of Scope)

Platform commission (taking a cut from sellers) is **not implemented in this plan** but the double-entry model is designed to support it cleanly when added:

**Current capture** (full amount to platform_revenue):
```
Debit:  platform_revenue  +$49.98
Credit: gateway_holding   +$49.98
```

**Future capture with commission** (split between platform and seller):
```
Debit:  platform_revenue  +$4.99    ← 10% commission
Credit: gateway_holding   +$4.99

Debit:  seller_payable    +$44.99   ← seller's share
Credit: gateway_holding   +$44.99
```

When commission is implemented, it will require:
- A `commission_rate` configuration (per-seller, per-category, or global)
- The capture flow splits into two entry pairs instead of one
- The `seller_payable` accounts become the source of truth for seller disbursements
- A disbursement flow (periodic payout to sellers) debits `seller_payable` and credits a new `seller_paid` account

**The key point**: the schema and account structure already support this. No migration needed — just business logic changes in the capture handler.

### 2.7 Payment State (Derived from Ledger)

State is computed from transactions, not stored:

```
Latest posted authorization → Authorized
Latest posted capture       → Captured
Latest posted void          → Voided
Latest posted refund        → Refunded
Latest pending transaction  → Pending
Latest discarded (no posted)→ Failed
```

The `payment_status` view provides convenience access. The `account_balances` view answers financial queries.

### 2.8 Mock Payment Gateway

```rust
#[async_trait]
pub trait PaymentGateway: Send + Sync {
    async fn authorize(&self, idempotency_key: &str, order_id: Uuid, amount: Decimal, currency: &str)
        -> Result<GatewayAuthResult, AppError>;
    async fn capture(&self, gateway_reference: &str) -> Result<GatewayCaptureResult, AppError>;
    async fn void(&self, gateway_reference: &str) -> Result<GatewayVoidResult, AppError>;
    async fn refund(&self, gateway_reference: &str, amount: Decimal)
        -> Result<GatewayRefundResult, AppError>;
}

pub struct MockPaymentGateway { success_rate: f64 }
// ::always_succeeds() / ::always_fails() for testing
// Simulated 50ms latency, random reference IDs
// Accepts idempotency_key on authorize()
```

Follows ADR-006 pattern (EmailService trait with mock). `idempotency_key` on `authorize()` prevents duplicate charges.

### 2.9 Gateway Timeout Handling

Config: `payment_timeout_seconds` (default 30s).

If authorization is pending beyond this timeout, the payment service writes a `PaymentTimedOut` event. The order service treats this as a failure and cancels. If the gateway later reports success asynchronously, the payment service checks if the order is already cancelled and issues an automatic void.

Flow:
```
InventoryReserved → [Payment: authorize starts] → (30s timeout)
  → PaymentTimedOut → [Order: cancel] → OrderCancelled → [Catalog: release]
  → (gateway late success) → [Payment: auto-void]
```

### 2.10 Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/payments/{order_id}` | Owner/Admin | Payment status + ledger entries for order |

Payment service is primarily event-driven. Most logic lives in consumers.

### 2.11 Kafka Consumers

- **inventory.events** (group: `payment-service`):
  - `InventoryReserved` → authorize payment via gateway → write `PaymentAuthorized` or `PaymentFailed` to outbox

- **orders.events** (group: `payment-service`):
  - `OrderConfirmed` → capture authorized payment
  - `OrderCancelled` → void (if authorized) or refund (if captured)

### 2.12 File Structure

```
payment/src/
├── main.rs, lib.rs
├── ledger/        accounts, transactions, entries — repository, entities, value_objects
├── payments/      routes, service, domain, dtos (orchestrates ledger operations)
├── gateway/       traits (PaymentGateway), mock (MockPaymentGateway)
├── outbox/        entities, repository, relay (via outbox-core)
├── consumers/     inventory_events, order_events
└── events/        types (PaymentAuthorized, PaymentFailed, PaymentTimedOut, PaymentCaptured, PaymentVoided)
```

### 2.13 Future Work (Out of Scope)

- **Platform commission**: Double-entry model supports it (see §2.6). Needs commission_rate config + capture split logic. No schema changes.
- **Seller disbursements**: Periodic payout from `seller_payable` accounts. Needs a disbursement scheduler + new account type (`seller_paid`). The ledger data is already there.
- **Saved payment methods**: Lives in identity service (user's saved cards) or a dedicated payment-methods service. Not payment service (which handles transactions, not card storage).
- **Subscriptions**: Ledger entries support recurring charges. Implementation deferred.
- **Seller refusal to fulfill**: Requires a seller fulfillment workflow (accept/reject order) — a post-confirmation feature. Currently `Confirmed → Shipped` is direct.

---

## 3. Catalog Inventory Extension

### 3.1 New Migration

```sql
ALTER TABLE skus ADD COLUMN reserved_quantity INTEGER NOT NULL DEFAULT 0;
ALTER TABLE skus ADD CONSTRAINT chk_reserved CHECK (reserved_quantity >= 0);
ALTER TABLE skus ADD CONSTRAINT chk_available CHECK (stock_quantity >= reserved_quantity);

-- Available stock view for read endpoints
CREATE VIEW sku_availability AS
SELECT id, stock_quantity, reserved_quantity,
       (stock_quantity - reserved_quantity) AS available_quantity
FROM skus;

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

- Available stock = `stock_quantity - reserved_quantity`
- `sku_availability` view used by product list/detail read endpoints to show available stock (simple subtraction on indexed columns, no materialized view needed)
- Separate `inventory_reservations` table tracks which order owns each reservation, enables targeted release, provides audit trail

### 3.2 Reservation Flow

1. `OrderCreated` event received
2. For each item: `SELECT sku FOR UPDATE` → check available → increment reserved → create reservation
3. All in single transaction (atomic — if any item fails, entire reservation fails)
4. Success → write `InventoryReserved` to outbox
5. Failure (any item insufficient) → write `InventoryReservationFailed` to outbox

**Concurrency**: `SELECT ... FOR UPDATE` provides row-level locking. Two concurrent reservations for the same SKU serialize at the DB level. This is the standard approach and sufficient for expected load.

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
- Product list/detail endpoints use `sku_availability` view for stock display

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

### PaymentTimedOut (topic: `payments.events`)
```json
{ "order_id": "uuid", "reason": "Gateway authorization timed out after 30s" }
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

### Failure: Payment Timeout
```
OrderCreated → InventoryReserved → [Payment] authorize starts → (30s timeout)
  → PaymentTimedOut → [Order] cancel → OrderCancelled → [Catalog] release inventory
  → (gateway late success) → [Payment] auto-void
```

### Manual Cancellation (from buyer side)
```
POST /orders/{id}/cancel → OrderCancelled
  → [Catalog] release inventory + [Payment] void/refund
```

---

## 6. Compensation Summary

| Trigger | Reactor | Action |
|---------|---------|--------|
| InventoryReservationFailed | Order | Cancel order |
| PaymentFailed | Order | Cancel order → OrderCancelled |
| PaymentTimedOut | Order | Cancel order → OrderCancelled |
| OrderCancelled (inv reserved) | Catalog | Release reserved stock |
| OrderCancelled (pay authorized) | Payment | Void transaction (discard auth entries, post void entries) |
| OrderCancelled (pay captured) | Payment | Refund transaction (post refund entries reversing capture) |
| Late gateway success after timeout | Payment | Auto-void transaction (discard late auth, post void entries) |

---

## 7. Resilience & Consistency

- **Cascading failure prevention**: Choreography saga with outbox — each service processes events independently. Failures are local; outbox buffers events when Kafka is down.
- **Money consistency**: Double-entry ensures every transaction balances (`sum(debits) == sum(credits)`). Discrepancies are structurally impossible within the ledger. Cross-service consistency (order status vs payment state) relies on saga events. `NUMERIC(19,4)` everywhere (ADR-007).
- **Reconciliation (future work)**: Periodic job to detect out-of-sync states (e.g., order Confirmed but no capture transaction). The double-entry model makes this queryable via `account_balances` view. Not building now but the data model supports it.

---

## 8. Testing (~160 total, per test-standards.md)

### Order Service (~65 tests)
- Value objects: status transitions, idempotency key, quantity (~12)
- Repository: CRUD, outbox entries, processed_events (~15)
- Service: create, handle events, idempotency, cancel, seller query (~20)
- Router: HTTP integration, auth, response shapes (~18)

### Payment Service (~55 tests)
- Value objects: account types, transaction types, entry directions (~8)
- Gateway: mock authorize/capture/void/refund, idempotency (~10)
- Repository: account creation, transaction + paired entries, status view, balance view, outbox (~15)
- Service: event handlers with success/fail/timeout mocks, entry pairing correctness, balance invariants (~14)
- Router: HTTP integration (~8)

### Catalog Inventory (~30 tests)
- Reserve stock, release, confirm (~10)
- Edge cases: insufficient stock, duplicate reservation, concurrent reservations (~8)
- Event handler integration (~7)
- Availability view used in product reads (~5)

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
Tests: happy path, inventory failure, payment failure, payment timeout, manual cancel.

Infra failure simulation: Use `MockPaymentGateway::always_fails()` for payment failures. Kafka failures handled by outbox (events stay in outbox). Skip chaos testing for now.

---

## 9. Implementation Order

1. Order: schema + repository + entities
2. Order: value objects + domain + DTOs
3. Order: service (create, get, list, cancel — no events yet)
4. Order: routes + router tests (including seller endpoint)
5. Order: outbox (via outbox-core) + relay
6. Payment: double-entry schema (accounts, transactions, entries) + repository
7. Payment: gateway trait + mock (with idempotency key)
8. Payment: service + event handlers (paired entries, timeout handling)
9. Payment: routes + balance/status views
10. Catalog: inventory migration (including sku_availability view) + repository
11. Catalog: inventory service + event handlers
12. Wire Kafka consumers in each main.rs
13. Saga integration tests
14. CLAUDE.md for order and payment

---

## 10. Cargo.toml (Order)

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
