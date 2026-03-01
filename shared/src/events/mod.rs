pub mod admin;
mod producer;
mod publisher;
mod types;

pub use admin::{KafkaAdmin, TopicSpec};
pub use producer::KafkaEventPublisher;
pub use publisher::EventPublisher;
pub use types::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};

#[cfg(feature = "test-utils")]
mod mock;
#[cfg(feature = "test-utils")]
pub use mock::MockEventPublisher;
