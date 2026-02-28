use crate::errors::AppError;
use crate::events::EventEnvelope;

/// Trait for publishing domain events to a message broker.
///
/// Implementations:
/// - `MockEventPublisher` (always available) — collects events for test assertions
/// - `KafkaEventPublisher` (feature: `kafka`) — publishes to Kafka via rdkafka
#[async_trait::async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, topic: &str, envelope: &EventEnvelope) -> Result<(), AppError>;
}
