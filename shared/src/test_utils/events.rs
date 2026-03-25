use uuid::Uuid;

use crate::events::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};

/// Derive (SourceService, AggregateType) from an EventType.
/// Covers every variant so callers never need to duplicate this mapping.
fn source_and_aggregate(event_type: &EventType) -> (SourceService, AggregateType) {
    match event_type {
        EventType::OrderCreated | EventType::OrderConfirmed | EventType::OrderCancelled => {
            (SourceService::Order, AggregateType::Order)
        }
        EventType::InventoryReserved
        | EventType::InventoryReservationFailed
        | EventType::InventoryReleased => (SourceService::Catalog, AggregateType::Inventory),
        EventType::PaymentAuthorized
        | EventType::PaymentFailed
        | EventType::PaymentCaptured
        | EventType::PaymentVoided
        | EventType::PaymentTimedOut
        | EventType::PaymentCaptureRetryRequested => {
            (SourceService::Payment, AggregateType::Payment)
        }
    }
}

/// Build an EventEnvelope for testing.
///
/// Automatically derives source_service and aggregate_type from the event_type.
/// `aggregate_id` is used as the partition key (typically order_id).
/// `extra` fields are merged into the payload alongside `"order_id"`.
pub fn make_envelope(
    event_type: EventType,
    aggregate_id: Uuid,
    extra: serde_json::Value,
) -> EventEnvelope {
    let (source, agg_type) = source_and_aggregate(&event_type);

    let mut payload = serde_json::json!({ "order_id": aggregate_id.to_string() });
    if let serde_json::Value::Object(map) = extra {
        for (k, v) in map {
            payload[k] = v;
        }
    }

    let metadata = EventMetadata::new(event_type, agg_type, aggregate_id, source);
    EventEnvelope::new(metadata, payload)
}
