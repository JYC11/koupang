use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which service produced the event.
/// Serialized as lowercase strings on the wire (e.g. "order").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SourceService {
    Identity,
    Catalog,
    Cart,
    Order,
    Payment,
    Shipping,
    Notification,
}

/// Aggregate types across all services.
/// Serialized as PascalCase strings on the wire (e.g. "Order").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AggregateType {
    Order,
    Payment,
    Inventory,
}

/// Event types across all services.
/// Serialized as PascalCase strings on the wire (e.g. "OrderCreated").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EventType {
    // Order events
    OrderCreated,
    OrderConfirmed,
    OrderCancelled,
    // Inventory events
    InventoryReserved,
    InventoryReservationFailed,
    InventoryReleased,
    // Payment events
    PaymentAuthorized,
    PaymentFailed,
    PaymentCaptured,
    PaymentVoided,
    PaymentTimedOut,
}

/// Metadata attached to every domain event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    /// Unique event identifier (UUID v7 for time-ordering).
    pub event_id: Uuid,
    /// What happened.
    pub event_type: EventType,
    /// Which aggregate this event belongs to.
    pub aggregate_type: AggregateType,
    /// The specific aggregate instance.
    pub aggregate_id: Uuid,
    /// When the event was produced.
    pub timestamp: DateTime<Utc>,
    /// Which service produced the event.
    pub source_service: SourceService,
    /// W3C traceparent / correlation ID for distributed tracing.
    pub correlation_id: Option<String>,
    /// The event_id of the event that caused this one.
    pub causation_id: Option<Uuid>,
}

/// Wire-format envelope: metadata + arbitrary JSON payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub metadata: EventMetadata,
    pub payload: serde_json::Value,
}

impl EventMetadata {
    pub fn new(
        event_type: EventType,
        aggregate_type: AggregateType,
        aggregate_id: Uuid,
        source_service: SourceService,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            event_type,
            aggregate_type,
            aggregate_id,
            timestamp: Utc::now(),
            source_service,
            correlation_id: None,
            causation_id: None,
        }
    }

    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn with_causation_id(mut self, causation_id: Uuid) -> Self {
        self.causation_id = Some(causation_id);
        self
    }
}

impl EventEnvelope {
    pub fn new(metadata: EventMetadata, payload: serde_json::Value) -> Self {
        Self { metadata, payload }
    }

    /// Convenience: the Kafka partition key (aggregate_id as string).
    pub fn partition_key(&self) -> String {
        self.metadata.aggregate_id.to_string()
    }

    /// Extract a UUID field from the payload. Common pattern in consumer handlers.
    pub fn payload_uuid(&self, field: &str) -> Result<Uuid, crate::errors::AppError> {
        self.payload[field]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                crate::errors::AppError::BadRequest(format!(
                    "Missing or invalid {field} in {} payload",
                    self.metadata.event_type
                ))
            })
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Serialize as the same PascalCase string serde uses
        write!(f, "{:?}", self)
    }
}

impl std::fmt::Display for AggregateType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::fmt::Display for SourceService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Identity => "identity",
            Self::Catalog => "catalog",
            Self::Cart => "cart",
            Self::Order => "order",
            Self::Payment => "payment",
            Self::Shipping => "shipping",
            Self::Notification => "notification",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_type_serializes_as_pascal_case() {
        let json = serde_json::to_string(&EventType::OrderCreated).unwrap();
        assert_eq!(json, "\"OrderCreated\"");

        let json = serde_json::to_string(&EventType::InventoryReservationFailed).unwrap();
        assert_eq!(json, "\"InventoryReservationFailed\"");
    }

    #[test]
    fn event_type_deserializes_from_pascal_case() {
        let et: EventType = serde_json::from_str("\"PaymentAuthorized\"").unwrap();
        assert_eq!(et, EventType::PaymentAuthorized);
    }

    #[test]
    fn aggregate_type_round_trip() {
        for agg in [
            AggregateType::Order,
            AggregateType::Payment,
            AggregateType::Inventory,
        ] {
            let json = serde_json::to_string(&agg).unwrap();
            let back: AggregateType = serde_json::from_str(&json).unwrap();
            assert_eq!(agg, back);
        }
    }

    #[test]
    fn event_type_all_variants_round_trip() {
        let variants = [
            EventType::OrderCreated,
            EventType::OrderConfirmed,
            EventType::OrderCancelled,
            EventType::InventoryReserved,
            EventType::InventoryReservationFailed,
            EventType::InventoryReleased,
            EventType::PaymentAuthorized,
            EventType::PaymentFailed,
            EventType::PaymentCaptured,
            EventType::PaymentVoided,
            EventType::PaymentTimedOut,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: EventType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn event_envelope_serialization_round_trip() {
        let aggregate_id = Uuid::now_v7();
        let metadata = EventMetadata::new(
            EventType::OrderCreated,
            AggregateType::Order,
            aggregate_id,
            SourceService::Order,
        )
        .with_correlation_id("trace-abc-123");

        let payload = json!({
            "order_id": aggregate_id.to_string(),
            "buyer_id": Uuid::now_v7().to_string(),
            "total": "129.99",
            "items": [{"sku_id": Uuid::now_v7().to_string(), "quantity": 2}]
        });

        let envelope = EventEnvelope::new(metadata, payload);

        let serialized = serde_json::to_string(&envelope).unwrap();
        let deserialized: EventEnvelope = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.metadata.event_type, EventType::OrderCreated);
        assert_eq!(deserialized.metadata.aggregate_type, AggregateType::Order);
        assert_eq!(deserialized.metadata.aggregate_id, aggregate_id);
        assert_eq!(deserialized.metadata.source_service, SourceService::Order);
        assert_eq!(
            deserialized.metadata.correlation_id.as_deref(),
            Some("trace-abc-123")
        );
        assert!(deserialized.metadata.causation_id.is_none());
    }

    #[test]
    fn partition_key_is_aggregate_id() {
        let aggregate_id = Uuid::now_v7();
        let metadata = EventMetadata::new(
            EventType::PaymentAuthorized,
            AggregateType::Payment,
            aggregate_id,
            SourceService::Payment,
        );
        let envelope = EventEnvelope::new(metadata, json!({}));
        assert_eq!(envelope.partition_key(), aggregate_id.to_string());
    }

    #[test]
    fn event_metadata_builder_methods() {
        let agg_id = Uuid::now_v7();
        let cause_id = Uuid::now_v7();

        let metadata = EventMetadata::new(
            EventType::InventoryReserved,
            AggregateType::Inventory,
            agg_id,
            SourceService::Catalog,
        )
        .with_correlation_id("corr-123")
        .with_causation_id(cause_id);

        assert_eq!(metadata.correlation_id.as_deref(), Some("corr-123"));
        assert_eq!(metadata.causation_id, Some(cause_id));
    }

    #[test]
    fn source_service_serializes_as_lowercase() {
        let json = serde_json::to_string(&SourceService::Order).unwrap();
        assert_eq!(json, "\"order\"");

        let json = serde_json::to_string(&SourceService::Catalog).unwrap();
        assert_eq!(json, "\"catalog\"");
    }

    #[test]
    fn source_service_all_variants_round_trip() {
        let variants = [
            SourceService::Identity,
            SourceService::Catalog,
            SourceService::Cart,
            SourceService::Order,
            SourceService::Payment,
            SourceService::Shipping,
            SourceService::Notification,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: SourceService = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn display_impls() {
        assert_eq!(EventType::OrderCreated.to_string(), "OrderCreated");
        assert_eq!(AggregateType::Order.to_string(), "Order");
        assert_eq!(SourceService::Order.to_string(), "order");
        assert_eq!(SourceService::Catalog.to_string(), "catalog");
    }

    #[test]
    fn invalid_event_type_fails_deserialization() {
        let result = serde_json::from_str::<EventType>("\"NonExistentEvent\"");
        assert!(result.is_err());
    }
}
