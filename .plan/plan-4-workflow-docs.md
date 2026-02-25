# Plan 4: Workflow Documentation

## Context

After Plans 1-3 are implemented, document the complete system for future sessions, portfolio presentation, and onboarding.

---

## 1. ADRs to Create

### ADR-010: Event-Driven Architecture with Kafka + Transactional Outbox
- **Decision**: Choreography saga with Kafka (KRaft), transactional outbox for guaranteed delivery
- **Alternatives rejected**: Orchestration saga (central coordinator), direct Kafka publish (no atomicity), rskafka (no consumer group rebalancing)
- **Key implication**: Every event-producing write requires outbox row in same DB transaction
- **Crate**: `rdkafka` (librdkafka wrapper), feature-gated behind `kafka`

### ADR-011: Redis-Only Service Bootstrap
- **Decision**: New `run_redis_service_with_infra()` for services without Postgres
- **Rationale**: Cart service needs Redis only; avoid wasting a Postgres connection
- **Key implication**: `RedisServiceConfig` (no db_url, no migrations)

### ADR-012: Mock Payment Gateway (Trait-Based DI)
- **Decision**: `PaymentGateway` trait with `MockPaymentGateway` default implementation
- **Pattern**: Same as `EmailService` trait (ADR-006)
- **Key implication**: Real gateway (Stripe) can be swapped in without changing business logic

### ADR-013: Inventory Reservation Design
- **Decision**: Separate `inventory_reservations` table + denormalized `reserved_quantity` on SKUs
- **Rationale**: Tracks per-order reservations, enables targeted release, provides audit trail
- **Key implication**: Available stock = stock_quantity - reserved_quantity

---

## 2. Saga Flow Documentation

File: `.plan/ordering-saga-flows.md`

### Contents
- Complete ASCII sequence diagrams (already designed in Plan 3):
  - Happy path: order → reserve → authorize → confirm → capture
  - Inventory failure: order → reservation failed → cancel
  - Payment failure: order → reserve → payment declined → cancel → release
  - Manual cancellation: cancel → release inventory + void/refund payment
- Compensation table: trigger → reactor → action
- Event payload specifications
- Topic routing table

---

## 3. Service CLAUDE.md Files

### `cart/CLAUDE.md`
- Architecture: Redis-only, no Postgres
- Endpoints table (6 endpoints)
- Redis data model (key pattern, TTL, max items)
- Value objects
- File layout
- Key imports
- Test structure

### `order/CLAUDE.md`
- Architecture: layered + event-driven
- Endpoints table (4 endpoints)
- Schema (orders, order_items, outbox, processed_events)
- State machine diagram
- Kafka consumers and topics
- Event types published
- File layout
- Test structure

### `payment/CLAUDE.md`
- Architecture: primarily event-driven
- Endpoints table (1 endpoint + webhook)
- Schema (payments, outbox, processed_events)
- Payment status state machine
- Mock gateway design
- Kafka consumers and topics
- Event types published
- File layout
- Test structure

---

## 4. Root CLAUDE.md Updates

### Services Table
Update status for:
- Cart: Complete (XX tests)
- Order: Complete (XX tests)
- Payment: Complete (XX tests)
- Catalog: note inventory extension

### ADR Summary Table
Add entries 010-013

### Tech Stack
Add Kafka to infrastructure section

### Key Shared Imports
Add event system imports:
```rust
use shared::events::{EventEnvelope, EventMetadata, DomainEvent};
use shared::events::outbox;
```

---

## 5. Shared CLAUDE.md Update

Add documentation for new modules:
- `events/` — Event types, Kafka producer/consumer, outbox, relay
- `idempotency/` (if implemented) — Middleware, repository
- Server extensions — `RedisServiceConfig`, event-driven service bootstrap

---

## 6. Progress Summary

File: `.plan/progress-summary-pt3.md`

### Contents
- Cart service summary: endpoints, Redis patterns, test count
- Order service summary: saga pattern, state machine, test count
- Payment service summary: mock gateway, event-driven, test count
- Catalog extension summary: inventory reservation, test count
- Infrastructure additions: Kafka, Jaeger, outbox relay
- Lessons learned / architectural notes

---

## 7. Implementation Order

1. `.plan/ordering-saga-flows.md` — saga documentation
2. ADRs 010-013 (via `make adr`)
3. `cart/CLAUDE.md`
4. `order/CLAUDE.md`
5. `payment/CLAUDE.md`
6. Update `shared/CLAUDE.md`
7. Update root `CLAUDE.md`
8. `.plan/progress-summary-pt3.md`
