# Payment Service

Double-entry ledger payment processing with gateway abstraction and saga integration.

## Architecture

- Double-entry ledger: accounts (buyer, gateway_holding, platform_revenue, seller_payable), transactions, entries
- Payment state derived from posted transactions: New → Authorized → Captured (or Voided/Refunded/Failed)
- Gateway abstraction: `PaymentGateway` trait with `MockPaymentGateway` for dev
- DOP rule algebra: `authorization_rules()` (4 checks), `capture_rules()` (2 checks)
- Claims-based JWT auth (ADR-008)
- Kafka consumers for inventory + order events via `PaymentEventHandler`

## File Layout

```
payment/src/
├── main.rs / lib.rs              # AppState { pool, auth_config }; Kafka consumer wired via ServiceBuilder
├── payments/                     # dtos.rs, error.rs, routes.rs, rules.rs, service.rs
├── ledger/                       # entities.rs, repository.rs, value_objects.rs — double-entry bookkeeping
├── gateway/                      # traits.rs (PaymentGateway), mock.rs (MockPaymentGateway)
├── consumers/                    # handler.rs (PaymentEventHandler), inventory_events.rs, order_events.rs
├── events/types.rs               # Event payload structs
└── outbox/                       # Uses shared outbox infrastructure
```

Tests: `tests/payments/{repository,service,router}_test.rs` + `tests/common/mod.rs`

## Endpoints (`/api/v1/payments`)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/{order_id}` | JWT | Payment status: state, transactions with entries, account balances |

## Ledger Model

- **Accounts**: per-order, per-type (buyer/gateway_holding/platform_revenue/seller_payable), per-currency
- **Transactions**: authorization, capture, void, refund — each with idempotency key
- **Entries**: debit/credit pairs per transaction (amount > 0 constraint)
- **Balances**: `account_balances` view — only counts entries from posted transactions

## Kafka Consumer (payments.events producer, catalog.events + orders.events consumer)

| Consumed Event | Handler | Action |
|---|---|---|
| `InventoryReserved` | `inventory_events` | Authorize payment via gateway, write ledger entries + PaymentAuthorized/PaymentFailed outbox |
| `OrderConfirmed` | `order_events` | Capture authorized payment, write PaymentCaptured outbox |
| `OrderCancelled` | `order_events` | Void if authorized (no-op if New/Failed) |

Payment service functions have `_on_tx` variants for use within the consumer's transaction:
`authorize_payment_on_tx`, `capture_payment_on_tx`, `void_payment_on_tx`.

## Key Patterns

- **3-case idempotency**: check existing transaction by idempotency key — already processed (skip), discarded (retry), new
- **Amount tamper detection**: if `gateway.approved_amount != requested`, void the auth and write PaymentFailed
- **Account balances**: view only sums entries from posted transactions; pending/discarded entries excluded

## Env Vars

`PAYMENT_DB_URL`, `PAYMENT_PORT` (default 3000), `KAFKA_BROKERS`, `ACCESS_TOKEN_SECRET`

## Tests

42 unit + 28 integration = 70 tests. `make test SERVICE=payment`
