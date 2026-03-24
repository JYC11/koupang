# Ordering Saga Flows

Choreography-based saga for the order-to-payment lifecycle. No central orchestrator — each
service consumes events and publishes its own via the transactional outbox pattern.

## Services Involved

| Service | Role | Consumes | Produces |
|---------|------|----------|----------|
| Order | Saga initiator, state machine | `catalog.events`, `payments.events` | `orders.events` |
| Catalog | Inventory reservation | `orders.events` | `catalog.events` |
| Payment | Authorization, capture, void | `catalog.events`, `orders.events`, `payments.events` (self) | `payments.events` |

## Event Types

```
Order events (orders.events topic):
  OrderCreated                    — order placed, saga begins
  OrderConfirmed                  — payment authorized, order auto-confirmed
  OrderCancelled                  — cancellation (any cause)

Inventory events (catalog.events topic):
  InventoryReserved               — stock reserved for order
  InventoryReservationFailed      — insufficient stock
  InventoryReleased               — reservation released (cancel path)

Payment events (payments.events topic):
  PaymentAuthorized               — gateway authorized the charge
  PaymentFailed                   — gateway declined or non-retryable error
  PaymentCaptured                 — charge captured (finalized)
  PaymentVoided                   — authorized charge reversed
  PaymentTimedOut                 — external timeout monitor (synthetic)
  PaymentCaptureRetryRequested    — self-consumed retry for capture failures
```

## Flow 1: Happy Path

```
  Order Service              Catalog Service              Payment Service
  ─────────────              ───────────────              ───────────────
  POST /orders
  ├─ create order (Pending)
  └─ outbox: OrderCreated ──────┐
                                │
                                ▼
                           handle_order_created
                           ├─ reserve inventory
                           └─ outbox: InventoryReserved ───────┐
                                │                               │
                                │                          (fan-out: same event)
                                │                               │
                                ▼                               ▼
                           [Order receives]              handle_inventory_reserved
                           Pending → InventoryReserved   ├─ gateway.authorize()
                                                         ├─ create ledger entries
                                                         └─ outbox: PaymentAuthorized ──┐
                                                                                        │
                                                                                        ▼
                                                                                   [Order receives]
                                                                                   InventoryReserved
                                                                                    → PaymentAuthorized
                                                                                    → Confirmed (auto)
                                                                                   outbox: OrderConfirmed ──┐
                                                                                                            │
                                                                                                            ▼
                                                                                                  handle_order_confirmed
                                                                                                  ├─ gateway.capture()
                                                                                                  └─ outbox: PaymentCaptured

  Final state:
    Order     = Confirmed
    Inventory = reserved_quantity incremented
    Payment   = Captured (auth + capture transactions, 4 ledger entries)
```

## Flow 2: Inventory Failure

```
  Order Service              Catalog Service
  ─────────────              ───────────────
  POST /orders
  └─ outbox: OrderCreated ──────┐
                                │
                                ▼
                           handle_order_created
                           ├─ reserve fails (insufficient stock)
                           ├─ InventoryReservationFailed written on SEPARATE tx
                           │  (survives consumer rollback — saga compensation pattern)
                           └─ returns Err → consumer rolls back partial reserves
                                │
                                ▼
                           outbox: InventoryReservationFailed ──┐
                                                                │
                                                                ▼
                                                           [Order receives]
                                                           handle_inventory_reservation_failed
                                                           Pending → Cancelled
                                                           (reason from payload)

  Final state:
    Order     = Cancelled (reason: "Insufficient stock for SKU ...")
    Inventory = unchanged (rollback)
    Payment   = no records
```

## Flow 3: Payment Decline

```
  [Steps 1-4 same as happy path: OrderCreated → InventoryReserved → Order=InventoryReserved]

  Payment Service                  Order Service               Catalog Service
  ───────────────                  ─────────────               ───────────────
  handle_inventory_reserved
  ├─ gateway.authorize() → DECLINED
  └─ outbox: PaymentFailed ────────────┐
                                       │
                                       ▼
                                  handle_payment_failed
                                  InventoryReserved → Cancelled
                                  outbox: OrderCancelled ──────────────┐
                                                                       │
                                                                       ▼
                                                                  handle_order_cancelled
                                                                  release_for_order_on_tx
                                                                  (reserved_quantity → 0)

  Final state:
    Order     = Cancelled (reason: gateway decline message)
    Inventory = released (reserved_quantity = 0)
    Payment   = New or Failed (no ledger entries on decline)
```

## Flow 4: Payment Timeout

```
  [Steps 1-4: OrderCreated → InventoryReserved → Order=InventoryReserved]

  Timeout Monitor (external)       Order Service               Catalog Service
  ──────────────────────────       ─────────────               ───────────────
  Detects stale InventoryReserved
  Publishes: PaymentTimedOut ──────────┐
                                       │
                                       ▼
                                  handle_payment_timed_out
                                  InventoryReserved → Cancelled
                                  outbox: OrderCancelled ──────────────┐
                                                                       │
                                                                       ├──→ Catalog: release inventory
                                                                       └──→ Payment: void if authorized
                                                                            (no-op if never authorized)

  Final state:
    Order     = Cancelled (reason: "Payment timed out")
    Inventory = released
    Payment   = Voided (if authorized) or New (if never reached)
```

## Flow 5: Manual Cancel (after capture)

```
  [Full happy path completes: Order=Confirmed, Payment=Captured]

  Buyer cancels order
  ├─ cancel_order() validates can_cancel(Confirmed) → true
  ├─ Confirmed → Cancelled
  └─ outbox: OrderCancelled ───────────────┐
                                            │
                                            ├──→ Catalog: handle_order_cancelled
                                            │    release_for_order_on_tx
                                            │
                                            └──→ Payment: handle_order_cancelled
                                                 reads state = Captured
                                                 → logs warning, no void (captured payments
                                                   need refund, not void)

  Final state:
    Order     = Cancelled
    Inventory = released (reserved_quantity = 0)
    Payment   = Captured (refund is a separate flow, not implemented yet)
```

## Flow 6: Cancel Races Confirm (edge case)

```
  [Steps 1-7: Order auto-confirmed, OrderConfirmed in outbox but NOT yet delivered to payment]

  Cancel arrives before OrderConfirmed delivery:
  ├─ Order: Confirmed → Cancelled
  └─ outbox: OrderCancelled ───────────────┐
                                            │
                                            ├──→ Catalog: release inventory
                                            │
                                            └──→ Payment: handle_order_cancelled
                                                 reads state = Authorized (capture never happened)
                                                 → gateway.void() → PaymentVoided

  Final state:
    Order     = Cancelled
    Inventory = released
    Payment   = Voided (authorization reversed)
```

## Flow 7: Capture Retry (retryable gateway failure)

```
  [Happy path through PaymentAuthorized → Order=Confirmed → OrderConfirmed delivered to payment]

  Payment Service (capture attempt)
  ─────────────────────────────────
  handle_order_confirmed
  ├─ gateway.capture() → TIMEOUT (retryable)
  ├─ outbox: PaymentCaptureRetryRequested { retry_count: 1, reason: "timed out" }
  └─ returns Ok (consumer commits — OrderConfirmed marked as processed)

  [Outbox relay publishes PaymentCaptureRetryRequested to payments.events]
  [Payment consumer receives it (self-consumption)]

  handle_capture_retry
  ├─ retry_count (1) < MAX_CAPTURE_RETRIES (10) → attempt capture
  ├─ gateway.capture() → SUCCESS
  └─ outbox: PaymentCaptured

  If gateway still down:
  ├─ gateway.capture() → TIMEOUT
  └─ outbox: PaymentCaptureRetryRequested { retry_count: 2 }
  [... repeats until recovery or max retries ...]

  If max retries exhausted (retry_count >= 10):
  └─ outbox: PaymentFailed { reason: "Capture failed after 10 retries" }

  Key property: retry events are in Postgres (outbox table), not in-memory.
  Survives restarts, container crashes, and prolonged gateway outages.
```

## Event Payload Schemas

```json
OrderCreated: {
  "order_id": "uuid", "buyer_id": "uuid",
  "total_amount": "decimal-string", "currency": "USD",
  "items": [{ "product_id": "uuid", "sku_id": "uuid", "quantity": N, "unit_price": "decimal-string" }]
}

InventoryReserved: {
  "order_id": "uuid", "buyer_id": "uuid",
  "total_amount": "decimal-string", "currency": "USD",
  "items": [{ "sku_id": "uuid", "quantity": N }]
}

InventoryReservationFailed: { "order_id": "uuid", "reason": "string" }

PaymentAuthorized: { "order_id": "uuid", "payment_id": "uuid", "gateway_reference": "string" }
PaymentFailed:     { "order_id": "uuid", "reason": "string" }
PaymentCaptured:   { "order_id": "uuid" }
PaymentVoided:     { "order_id": "uuid" }

OrderConfirmed:  { "order_id": "uuid", "buyer_id": "uuid" }
OrderCancelled:  { "order_id": "uuid", "reason": "string" }

PaymentCaptureRetryRequested: { "order_id": "uuid", "retry_count": N, "reason": "string" }
```

## Infrastructure

### Transactional Outbox
Events written to `outbox_events` table in the same DB transaction as business changes.
Background `OutboxRelay` claims pending events (FOR UPDATE SKIP LOCKED) and publishes to Kafka.
Per-aggregate ordering via DISTINCT ON (aggregate_id).

### Consumer Idempotency
`processed_events` table tracks which events have been handled. Consumer wraps handler
in a transaction: check processed → handle → mark processed → commit → ack Kafka offset.

### Failure Event Survival (Catalog)
When inventory reservation fails, `InventoryReservationFailed` is written on a SEPARATE
pool transaction (not the consumer's tx). The consumer's tx rolls back (no partial reserves
committed), but the failure event survives to notify the order service.

### Distributed Lock (Payment)
Redis SETNX with TTL acquired per-order before payment handler dispatch. Prevents concurrent
authorize/capture/void for the same order. Fail-open: if Redis unavailable, proceeds without
lock (idempotency key at DB level catches duplicates; double gateway calls are the risk).

### Capture Retry via Outbox
On retryable capture failure, `capture_payment_on_tx` returns Ok and writes
`PaymentCaptureRetryRequested` to outbox. Consumer commits (original event processed).
Outbox relay publishes retry event → payment self-consumes → tries again. More resilient
than consumer-level retry (persistent in Postgres, survives prolonged gateway outages).

## Order State Machine

```
Pending ──────────────────────────────────────────── InventoryReserved
   │                                                       │
   │ (InventoryReservationFailed)                          │ (PaymentAuthorized)
   ▼                                                       ▼
Cancelled ◄────────────────────────────────────── PaymentAuthorized
   ▲                                                       │
   │ (PaymentFailed/TimedOut/ManualCancel)                  │ (auto)
   │                                                       ▼
   ├───────────────────────────────────────────── Confirmed
   │                                                       │
   │ (ManualCancel)                                        │
   │                                                       ▼
   ├─────────────────────────────────────────────── Shipped
   │                                                       │
   │                                                       ▼
   │                                                 Delivered
   │                                                       │
   │                                                       ▼
   │                                                  Returned
   │
   └── Terminal: Cancelled (from Pending, InventoryReserved, PaymentAuthorized, Confirmed, Shipped)
```

## Test Coverage

6 saga integration tests in `saga-tests/` verify the full event chain across all 3 services:
1. Happy path (OrderCreated → ... → PaymentCaptured)
2. Inventory failure (insufficient stock → OrderCancelled)
3. Payment decline (gateway decline → OrderCancelled + inventory released)
4. Payment timeout (synthetic timeout → OrderCancelled + inventory released)
5. Cancel after capture (manual cancel → inventory released, payment stays Captured)
6. Cancel races confirm (cancel before capture delivery → PaymentVoided)

Each test uses real Postgres databases (testcontainers), events threaded manually between
handler functions (no Kafka needed for correctness tests).
