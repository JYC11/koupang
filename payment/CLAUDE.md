# Payment Service

Double-entry ledger payment processing with gateway abstraction and saga integration.

## Architecture

- Double-entry ledger: accounts (buyer, gateway_holding, platform_revenue, seller_payable), transactions, entries
- Payment state derived from posted transactions: New → Authorized → Captured (or Voided/Refunded/Failed)
- Gateway abstraction: `PaymentGateway` trait with `GatewayError { code, message, reason, is_retryable }` and `MockPaymentGateway` for dev
- DOP rule algebra: `authorization_rules()` (4 checks), `capture_rules()` (2 checks)
- Claims-based JWT auth (ADR-008)
- Kafka consumers for inventory + order + self events via `PaymentEventHandler`
- Distributed lock (Redis, fail-open) prevents concurrent operations on the same order

## File Layout

```
payment/src/
├── main.rs / lib.rs              # AppState { pool, auth_config }; Kafka consumer wired via ServiceBuilder
├── payments/                     # dtos.rs, error.rs, routes.rs, rules.rs, service.rs
├── ledger/                       # entities.rs, repository.rs, value_objects.rs — double-entry bookkeeping
├── gateway/                      # traits.rs (PaymentGateway), mock.rs (MockPaymentGateway)
├── consumers/                    # handler.rs (PaymentEventHandler + distributed lock), inventory_events.rs, order_events.rs, capture_retry.rs
├── events/types.rs               # Event payload structs
└── outbox/                       # Uses shared outbox infrastructure
```

Tests: `tests/payments/{repository,service,router,consumer}_test.rs` + `tests/common/mod.rs`

## Endpoints (`/api/v1/payments`)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/{order_id}` | JWT | Payment status: state, transactions with entries, account balances |

## Ledger Model

- **Accounts**: per-order, per-type (buyer/gateway_holding/platform_revenue/seller_payable), per-currency
- **Transactions**: authorization, capture, void, refund — each with idempotency key
- **Entries**: debit/credit pairs per transaction (amount > 0 constraint)
- **Balances**: `account_balances` view — only counts entries from posted transactions

## Kafka Consumer (payments.events producer, catalog.events + orders.events + payments.events consumer)

| Consumed Event | Handler | Action |
|---|---|---|
| `InventoryReserved` | `inventory_events` | Authorize payment via gateway, write ledger entries + PaymentAuthorized/PaymentFailed outbox |
| `OrderConfirmed` | `order_events` | Capture authorized payment; on retryable failure writes `PaymentCaptureRetryRequested` to outbox |
| `OrderCancelled` | `order_events` | Void if authorized (no-op if New/Failed) |
| `PaymentCaptureRetryRequested` | `capture_retry` | Self-consumed retry: attempt capture again, re-queue on failure, `PaymentFailed` at MAX_CAPTURE_RETRIES (10) |

The `_on_tx` variants use a **read-call-write split**: reads on pool (connection released before gateway HTTP call), gateway call (no DB held), writes on consumer's tx. `PaymentEventHandler` holds `PgPool` (reads), `Arc<dyn PaymentGateway>` (gateway), and optional `DistributedLock` (concurrency guard).

**Distributed lock**: Acquired per-order before handler dispatch. `AlreadyHeld` → transient error (consumer retries). Redis unavailable → proceed without lock (fail-open, logged).

## Key Patterns

- **3-case idempotency**: check existing transaction by idempotency key — already processed (skip), discarded (retry), new
- **Amount tamper detection**: if `gateway.approved_amount != requested`, void the auth and write PaymentFailed
- **Account balances**: view only sums entries from posted transactions; pending/discarded entries excluded
- **GatewayError classification**: `is_retryable` flag distinguishes infra failures (timeout, 503) from business declines (card declined). Drives retry logic and future circuit breaker.
- **Capture retry via outbox**: On retryable capture failure, `capture_payment_on_tx` returns Ok and writes `PaymentCaptureRetryRequested` to outbox. More resilient than consumer retry — survives prolonged gateway outages.

## Env Vars

`PAYMENT_DB_URL`, `PAYMENT_PORT` (default 3000), `KAFKA_BROKERS`, `ACCESS_TOKEN_SECRET`, `REDIS_URL` (optional, for distributed lock)

## Tests

46 unit + 42 integration = 88 tests. `make test SERVICE=payment`

Test layers:
- Ledger repository (14): accounts, transactions, entries, balances view, pending exclusion
- Service (8): authorize/capture/void flows, tamper detection, rules
- Router (4): payment status endpoint, auth
- Consumer handlers (14): authorize, capture, void, cancel, lifecycle, capture retry (recovery, re-queue, max retries, retryable/non-retryable failures)
- Gateway mock (2): success/fail modes
- Error mapping (9): GatewayDeclined, GatewayRetryable, From<GatewayError>, tampered, infra passthrough
