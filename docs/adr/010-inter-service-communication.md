# ADR-010: Inter-Service Communication

**Date:** 2026-03-04
**Status:** Accepted

## Context

As we build services beyond identity and catalog, services will need to communicate. The order service needs product/stock data from catalog, the payment service needs order data, shipping needs order + payment confirmation, etc. We need a consistent rule for when to use synchronous vs asynchronous communication.

## Decision

- **Synchronous (REST)** for queries — when a service needs to *read* data owned by another service to fulfill a request (e.g., order service checks product price from catalog). Use `reqwest` with circuit breaker (`failsafe`).
- **Asynchronous (Kafka events)** for state changes — when a service's state transition should *notify* other services (e.g., `order.order.placed` triggers payment processing). Uses the transactional outbox pattern (already implemented in `shared`).
- **Never shared database** — services do not read each other's tables. Each service owns its data exclusively.
- **Event-carried state transfer** — events carry enough data for consumers to act without calling back to the producer. Avoids synchronous callback chains.
- **Idempotent consumers** — all event handlers must be idempotent (use event ID for deduplication).

### Communication Matrix

| Producer → Consumer | Method | Example |
|---------------------|--------|---------|
| Order → Payment | Event | `order.order.placed` triggers payment creation |
| Payment → Order | Event | `payment.payment.completed` updates order status |
| Order → Catalog | REST | Validate product exists, check price at order time |
| Order → Shipping | Event | `order.order.paid` triggers shipment creation |
| Shipping → Order | Event | `shipping.shipment.delivered` completes order |
| Any → Notification | Event | State changes trigger notification templates |

## Consequences

- **Easier:** Clear rule for choosing communication style. No ambiguity during implementation.
- **Harder:** Event-carried state transfer means events are larger (carry denormalized data). Schema evolution must be managed carefully (see ADR-011).
- **Trade-off:** REST queries create runtime coupling — if catalog is down, order creation fails. Circuit breaker + retry mitigate but don't eliminate this. Acceptable for read-path; critical writes always go through events.
