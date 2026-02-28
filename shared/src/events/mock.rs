use std::sync::{Arc, Mutex};

use crate::errors::AppError;
use crate::events::{EventEnvelope, EventPublisher};

/// Test double that captures published events for assertions.
///
/// # Usage
/// ```ignore
/// let publisher = MockEventPublisher::new();
/// // ... pass Arc<publisher> into service under test ...
/// let events = publisher.events();
/// assert_eq!(events.len(), 1);
/// assert_eq!(events[0].0, "orders.events");
/// ```
#[derive(Clone)]
pub struct MockEventPublisher {
    events: Arc<Mutex<Vec<(String, EventEnvelope)>>>,
}

impl MockEventPublisher {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns a snapshot of all published (topic, envelope) pairs.
    pub fn events(&self) -> Vec<(String, EventEnvelope)> {
        self.events.lock().unwrap().clone()
    }

    /// Number of published events.
    pub fn event_count(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Clear all captured events.
    pub fn clear(&self) {
        self.events.lock().unwrap().clear();
    }

    /// Returns events filtered by topic.
    pub fn events_for_topic(&self, topic: &str) -> Vec<EventEnvelope> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| t == topic)
            .map(|(_, e)| e.clone())
            .collect()
    }
}

impl Default for MockEventPublisher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl EventPublisher for MockEventPublisher {
    async fn publish(&self, topic: &str, envelope: &EventEnvelope) -> Result<(), AppError> {
        tracing::info!(
            topic = %topic,
            event_type = %envelope.metadata.event_type,
            aggregate_id = %envelope.metadata.aggregate_id,
            "[MOCK EVENT] Would publish event"
        );
        self.events
            .lock()
            .unwrap()
            .push((topic.to_string(), envelope.clone()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AggregateType, EventMetadata, EventType, SourceService};
    use serde_json::json;
    use uuid::Uuid;

    fn test_envelope(event_type: EventType, aggregate_type: AggregateType) -> EventEnvelope {
        let metadata = EventMetadata::new(
            event_type,
            aggregate_type,
            Uuid::now_v7(),
            SourceService::Order,
        );
        EventEnvelope::new(metadata, json!({"test": true}))
    }

    #[tokio::test]
    async fn captures_published_events() {
        let publisher = MockEventPublisher::new();

        publisher
            .publish(
                "orders.events",
                &test_envelope(EventType::OrderCreated, AggregateType::Order),
            )
            .await
            .unwrap();

        publisher
            .publish(
                "orders.events",
                &test_envelope(EventType::OrderConfirmed, AggregateType::Order),
            )
            .await
            .unwrap();

        assert_eq!(publisher.event_count(), 2);
        let events = publisher.events();
        assert_eq!(events[0].0, "orders.events");
        assert_eq!(events[0].1.metadata.event_type, EventType::OrderCreated);
        assert_eq!(events[1].1.metadata.event_type, EventType::OrderConfirmed);
    }

    #[tokio::test]
    async fn events_for_topic_filters_correctly() {
        let publisher = MockEventPublisher::new();

        publisher
            .publish(
                "orders.events",
                &test_envelope(EventType::OrderCreated, AggregateType::Order),
            )
            .await
            .unwrap();

        publisher
            .publish(
                "payments.events",
                &test_envelope(EventType::PaymentAuthorized, AggregateType::Payment),
            )
            .await
            .unwrap();

        publisher
            .publish(
                "orders.events",
                &test_envelope(EventType::OrderCancelled, AggregateType::Order),
            )
            .await
            .unwrap();

        let order_events = publisher.events_for_topic("orders.events");
        assert_eq!(order_events.len(), 2);
        assert_eq!(order_events[0].metadata.event_type, EventType::OrderCreated);
        assert_eq!(order_events[1].metadata.event_type, EventType::OrderCancelled);

        let payment_events = publisher.events_for_topic("payments.events");
        assert_eq!(payment_events.len(), 1);
        assert_eq!(payment_events[0].metadata.event_type, EventType::PaymentAuthorized);

        let inventory_events = publisher.events_for_topic("inventory.events");
        assert_eq!(inventory_events.len(), 0);
    }

    #[tokio::test]
    async fn clear_removes_all_events() {
        let publisher = MockEventPublisher::new();

        publisher
            .publish(
                "orders.events",
                &test_envelope(EventType::OrderCreated, AggregateType::Order),
            )
            .await
            .unwrap();

        assert_eq!(publisher.event_count(), 1);
        publisher.clear();
        assert_eq!(publisher.event_count(), 0);
        assert!(publisher.events().is_empty());
    }

    #[tokio::test]
    async fn is_send_and_sync() {
        let publisher = MockEventPublisher::new();
        // Prove it satisfies the trait bound Send + Sync
        let publisher: Arc<dyn EventPublisher> = Arc::new(publisher);

        let handle = tokio::spawn(async move {
            publisher
                .publish(
                    "orders.events",
                    &test_envelope(EventType::OrderCreated, AggregateType::Order),
                )
                .await
                .unwrap();
        });

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn clone_shares_state() {
        let publisher = MockEventPublisher::new();
        let clone = publisher.clone();

        publisher
            .publish(
                "orders.events",
                &test_envelope(EventType::OrderCreated, AggregateType::Order),
            )
            .await
            .unwrap();

        // Clone sees the same events
        assert_eq!(clone.event_count(), 1);
    }
}
