# Saga Integration Tests Plan — COMPLETED

## Status: Done (6 tests passing)

## Goal
Add a `saga-tests/` workspace crate that tests the full saga event chain across order, catalog, and payment services using event-threading simulation (no Kafka needed).

## Visibility Audit: No Changes Needed
- All handler functions are `pub`
- All `consumers` modules are `pub mod` in each service's `lib.rs`
- `MockPaymentGateway` is `pub` in `payment::gateway::mock`
- `PaymentGateway` trait is `pub` in `payment::gateway::traits`

## Handler Signatures (what saga-tests calls directly)

| Service | Handler | Signature |
|---------|---------|-----------|
| Catalog | `handle_order_created` | `(tx, pool, envelope) -> Result<(), HandlerError>` |
| Catalog | `handle_order_cancelled` | `(tx, envelope) -> Result<(), AppError>` |
| Order | `handle_inventory_reserved` | `(tx, envelope) -> Result<(), AppError>` |
| Order | `handle_inventory_reservation_failed` | `(tx, envelope) -> Result<(), AppError>` |
| Order | `handle_payment_authorized` | `(tx, envelope) -> Result<(), AppError>` |
| Order | `handle_payment_failed` | `(tx, envelope) -> Result<(), AppError>` |
| Order | `handle_payment_timed_out` | `(tx, envelope) -> Result<(), AppError>` |
| Payment | `handle_inventory_reserved` | `(tx, pool, gateway, envelope) -> Result<(), AppError>` |
| Payment | `handle_order_confirmed` | `(tx, pool, gateway, envelope) -> Result<(), AppError>` |
| Payment | `handle_order_cancelled` | `(tx, pool, gateway, envelope) -> Result<(), AppError>` |

## Architecture

```
saga-tests/
  Cargo.toml
  tests/
    common/
      mod.rs          # SagaHarness, OutboxDrainer, seed helpers, make_envelope
    saga_test.rs      # 5 scenarios
```

### SagaHarness
- 3 TestDb instances (shared Postgres container, separate databases with each service's migrations)
- MockPaymentGateway (controllable: `always_succeeds()` / `always_fails()`)
- `drain_outbox(pool, event_type) -> Option<EventEnvelope>`: reads pending outbox row, deserializes payload+metadata
- `deliver_to_catalog(envelope)`: calls `catalog::consumers::order_events::handle_*` with catalog pool
- `deliver_to_order(envelope)`: calls `order::consumers::*_events::handle_*` with order pool
- `deliver_to_payment(envelope)`: calls `payment::consumers::*_events::handle_*` with payment pool + gateway

### Seeding
Each test seeds:
1. **Catalog DB**: product + SKU with stock (so inventory reservation succeeds)
2. **Order DB**: order in Pending status with items referencing catalog's SKU IDs
3. **Payment DB**: nothing needed (accounts created on-the-fly by authorize flow)

The order's `items` payload must contain `sku_id`s that exist in catalog's DB.

### OutboxDrainer
```sql
SELECT * FROM outbox_events
WHERE event_type = $1 AND status = 'pending'
ORDER BY created_at ASC
LIMIT 1
```
Parse `payload` (already JSON) + reconstruct `EventEnvelope` from row fields. The outbox row has: `event_type`, `aggregate_type`, `aggregate_id`, `event_id`, `payload`, `metadata`.

## 5 Test Scenarios

### 1. Happy Path
```
Seed: catalog SKU (stock=100), order (Pending, qty=2)
Step 1: drain order outbox -> OrderCreated
Step 2: deliver to catalog -> handle_order_created
Step 3: drain catalog outbox -> InventoryReserved
Step 4: deliver to order  -> handle_inventory_reserved
Step 5: deliver to payment -> handle_inventory_reserved (authorize)
Step 6: drain payment outbox -> PaymentAuthorized
Step 7: deliver to order  -> handle_payment_authorized (auto-confirms, writes OrderConfirmed)
Step 8: drain order outbox -> OrderConfirmed
Step 9: deliver to payment -> handle_order_confirmed (capture)

Assert:
- Order DB: status = Confirmed
- Catalog DB: reserved_quantity = 2, stock unchanged
- Payment DB: state = Captured (authorization + capture transactions, 4 ledger entries)
- Outbox chain: OrderCreated -> InventoryReserved -> PaymentAuthorized -> OrderConfirmed -> PaymentCaptured
```

### 2. Inventory Failure (insufficient stock)
```
Seed: catalog SKU (stock=1), order (Pending, qty=5)
Step 1: drain order outbox -> OrderCreated
Step 2: deliver to catalog -> handle_order_created (fails, writes InventoryReservationFailed on pool)
Step 3: drain catalog outbox -> InventoryReservationFailed
Step 4: deliver to order  -> handle_inventory_reservation_failed

Assert:
- Order DB: status = Cancelled, reason = insufficient stock message
- Catalog DB: reserved_quantity = 0, stock unchanged
- Payment DB: no records
```

### 3. Payment Failure (gateway decline)
```
Seed: catalog SKU (stock=100), order (Pending, qty=2), gateway = always_fails()
Step 1-4: same as happy path up to InventoryReserved delivered to order
Step 5: deliver to payment -> handle_inventory_reserved (gateway declines -> PaymentFailed)
Step 6: drain payment outbox -> PaymentFailed
Step 7: deliver to order  -> handle_payment_failed (cancels, writes OrderCancelled)
Step 8: drain order outbox -> OrderCancelled
Step 9: deliver to catalog -> handle_order_cancelled (release inventory)

Assert:
- Order DB: status = Cancelled, reason contains "declined" or "failed"
- Catalog DB: reserved_quantity = 0 (released)
- Payment DB: state = Failed (no ledger entries or failed transaction)
```

### 4. Payment Timeout
```
Same as #3 but use PaymentTimedOut event instead of PaymentFailed.
(PaymentTimedOut is produced externally, e.g. by a timeout monitor — we simulate it)

Seed: catalog SKU (stock=100), order in InventoryReserved state
Step 1: make PaymentTimedOut envelope manually
Step 2: deliver to order -> handle_payment_timed_out (cancels, writes OrderCancelled)
Step 3: drain order outbox -> OrderCancelled
Step 4: deliver to catalog -> handle_order_cancelled (release)
Step 5: deliver to payment -> handle_order_cancelled (void if authorized, no-op if not)

Assert:
- Order DB: status = Cancelled, reason = "Payment timed out"
- Catalog DB: reserved_quantity = 0
```

### 5. Manual Cancel (after confirmation)
```
Seed: Run happy path steps 1-7 (order confirmed)
Step 1: Manually cancel order via order service (update status to Cancelled, write OrderCancelled)
Step 2: drain order outbox -> OrderCancelled
Step 3: deliver to catalog -> handle_order_cancelled (release)
Step 4: deliver to payment -> handle_order_cancelled (void authorized payment)

Assert:
- Order DB: status = Cancelled
- Catalog DB: reserved_quantity = 0
- Payment DB: state = Voided
```

## Tasks

1. Create `saga-tests/Cargo.toml` with deps on order, catalog, payment, shared (test-utils)
2. Add `saga-tests` to workspace members in root `Cargo.toml`
3. Implement `tests/common/mod.rs` — SagaHarness, drain_outbox, seed helpers, envelope builder
4. Implement 5 test scenarios in `tests/saga_test.rs`
5. Run tests, fix compilation + failures
6. Create lower-priority filament task for future E2E Kafka smoke tests

## Risks / Open Questions

- **TestDb isolation**: 3 `TestDb::start()` calls with different migration dirs — each creates a unique DB on the shared container. Should work but need to verify migration paths are correct from the saga-tests working directory (may need `../order/migrations`).
- **Catalog failure path**: `handle_order_created` returns `HandlerError` (not `AppError`). On inventory failure it writes the failure event on a **separate pool tx**, then returns `Err(HandlerError::permanent(...))`. Our test must: (a) expect the error return, (b) still find InventoryReservationFailed in the outbox (written on pool, survives the error).
- **PaymentTimedOut**: This event isn't produced by any service handler — it would come from an external timeout monitor. We'll construct it manually.
- **Manual cancel path**: Need to verify the order service has a way to cancel a Confirmed order and write OrderCancelled. Looking at the service, `cancel_order` checks `can_cancel()` — need to verify Confirmed is cancellable.
