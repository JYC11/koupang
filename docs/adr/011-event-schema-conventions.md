# ADR-011: Event Schema Conventions

**Date:** 2026-03-04
**Status:** Accepted

## Context

With the transactional outbox in place and inter-service communication decided (ADR-010), we need a standard for event naming, payload structure, and schema evolution so that producers and consumers stay compatible as services evolve.

## Decision

### Event Naming

Format: `{service}.{entity}.{past_tense_verb}`

Examples:
- `order.order.placed`
- `order.order.cancelled`
- `payment.payment.completed`
- `payment.payment.failed`
- `shipping.shipment.dispatched`
- `shipping.shipment.delivered`
- `catalog.product.created`
- `catalog.product.price_changed`

### Envelope Structure

Every event published through the outbox follows this envelope:

```json
{
  "event_id": "uuid-v7",
  "event_type": "order.order.placed",
  "aggregate_type": "order",
  "aggregate_id": "uuid-v7",
  "version": 1,
  "occurred_at": "2026-03-04T12:00:00Z",
  "trace_id": "opentelemetry-trace-id",
  "payload": { ... }
}
```

- `event_id` — unique per event, used for idempotency deduplication
- `version` — integer, incremented on breaking payload changes
- `trace_id` — propagated from the originating request for distributed tracing
- `payload` — event-specific data (see event-carried state transfer in ADR-010)

### Kafka Topics

One topic per aggregate type: `koupang.{service}.{aggregate}`

Examples:
- `koupang.order.order`
- `koupang.payment.payment`
- `koupang.catalog.product`

Consumer groups: `{consuming_service}.{aggregate}.consumer`

### Schema Evolution Rules

- **Additive only** — new fields are always optional with defaults. Never remove or rename fields in the same version.
- **Breaking changes** — increment `version`, support both old and new versions during transition, deprecate old version after all consumers migrate.
- **Payload should be self-contained** — carry enough data for consumers to act without calling back to the producer.

## Consequences

- **Easier:** Consistent naming makes topic discovery and event handler routing predictable. Envelope provides standard metadata for tracing and idempotency.
- **Harder:** Self-contained payloads mean larger events and potential data staleness (consumer has a snapshot, not live data). Acceptable trade-off for decoupling.
- **Future:** Consider a schema registry (e.g., Apache Avro) if payload complexity grows significantly.
