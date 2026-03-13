pub mod admin;
pub mod consumer;
pub mod health;
mod producer;
mod publisher;
mod types;

pub use crate::config::consumer_config::ConsumerConfig;
pub use admin::{KafkaAdmin, TopicSpec};
pub use consumer::{EventHandler, HandlerError, KafkaEventConsumer};
pub use health::{KafkaHealth, KafkaHealthChecker, KafkaHealthStatus};
pub use producer::KafkaEventPublisher;
pub use publisher::EventPublisher;
pub use types::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};

#[cfg(feature = "test-utils")]
mod mock;
#[cfg(feature = "test-utils")]
pub use mock::MockEventPublisher;

#[cfg(feature = "test-utils")]
mod mock_handler;
#[cfg(feature = "test-utils")]
pub use mock_handler::MockEventHandler;
