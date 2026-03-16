# Order Service

Order lifecycle management with state machine, idempotency, and saga event integration.

## Architecture

- Layered: `routes` → `service` (free fns) → `domain` → `repository` (free fns) → DB
- State machine: Pending → InventoryReserved → PaymentAuthorized → Confirmed → Shipped → Delivered (Cancelled/Returned as terminal)
- DOP rule algebra: `creation_rules()` (6 checks), `cancellation_rules()` (1 check)
- Claims-based JWT auth (ADR-008)
- Kafka consumers for inventory + payment events via `OrderEventHandler`

## File Layout

```
order/src/
├── main.rs / lib.rs              # AppState { pool, auth_config }; Kafka consumer wired via ServiceBuilder
├── orders/                       # dtos.rs, entities.rs, error.rs, repository.rs, routes.rs, rules.rs, service.rs, value_objects.rs
├── consumers/                    # handler.rs (OrderEventHandler), inventory_events.rs, payment_events.rs
├── events/types.rs               # Event payload structs (OrderCreated, InventoryReserved, PaymentAuthorized, etc.)
└── outbox/                       # Uses shared outbox infrastructure
```

Tests: `tests/orders/{repository,service,router,consumer}_test.rs` + `tests/common/mod.rs`

## Endpoints (`/api/v1/orders`)

All routes require JWT auth.

| Method | Path | Description |
|--------|------|-------------|
| POST | `/` | Create order (requires `Idempotency-Key` header, returns 202) |
| GET | `/` | List buyer's orders (keyset pagination, optional status filter) |
| GET | `/{id}` | Get order detail with items (owner-only via `require_access`) |
| GET | `/seller/me` | List orders containing seller's items |
| POST | `/{id}/cancel` | Cancel order (body: `{ "reason": "..." }`) |

## Value Objects

| VO | Rules |
|----|-------|
| `OrderStatus` | 8 states, `transition_to()` validates allowed transitions, `can_cancel()` predicate |
| `ShippingAddress` | street (max 500), city (max 200), postal_code (max 20), country (max 3) |
| `IdempotencyKey` | Non-empty, max 255 chars |
| `Quantity` | 1–9999 |
| `Price`, `Currency` | Re-exported from `shared::new_types::money` |

## Kafka Consumer (orders.events producer, catalog.events + payments.events consumer)

| Consumed Event | Handler | Action |
|---|---|---|
| `InventoryReserved` | `inventory_events` | Transition to InventoryReserved |
| `InventoryReservationFailed` | `inventory_events` | Cancel order with reason |
| `PaymentAuthorized` | `payment_events` | Transition to PaymentAuthorized → auto-Confirmed, write OrderConfirmed outbox |
| `PaymentFailed` | `payment_events` | Cancel order, write OrderCancelled outbox |
| `PaymentTimedOut` | `payment_events` | Cancel order, write OrderCancelled outbox |

All handlers use the consumer-provided `&mut PgConnection` (no own transactions or idempotency checks). All cancellation paths validate `transition_to(&Cancelled)` to prevent state machine corruption from duplicate events.

## Env Vars

`ORDER_DB_URL`, `ORDER_PORT` (default 3000), `KAFKA_BROKERS`, `ACCESS_TOKEN_SECRET`

## Tests

48 unit + 40 integration = 88 tests. `make test SERVICE=order`

Test layers:
- Repository (12): create, idempotency, status updates, keyset pagination, seller view
- Service (14): create with outbox, cancel with rules, access guards, idempotency
- Router (5): HTTP status codes, idempotency header, auth
- Consumer handlers (9): inventory reserved/failed transitions, payment authorized/failed/timed_out
