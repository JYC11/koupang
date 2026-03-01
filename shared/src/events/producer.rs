use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::message::{Header, OwnedHeaders};
use rdkafka::producer::{FutureProducer, FutureRecord};

use crate::config::kafka_config::KafkaConfig;
use crate::errors::AppError;
use crate::events::{EventEnvelope, EventPublisher};

/// Publishes domain events to Kafka as JSON with structured headers.
pub struct KafkaEventPublisher {
    producer: FutureProducer,
}

impl KafkaEventPublisher {
    pub fn new(config: &KafkaConfig) -> Result<Self, AppError> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|e| AppError::InternalServerError(format!("Kafka producer init: {e}")))?;

        Ok(Self { producer })
    }
}

#[async_trait::async_trait]
impl EventPublisher for KafkaEventPublisher {
    async fn publish(&self, topic: &str, envelope: &EventEnvelope) -> Result<(), AppError> {
        let payload = serde_json::to_string(envelope)
            .map_err(|e| AppError::InternalServerError(format!("Event serialize: {e}")))?;

        let key = envelope.partition_key();
        let m = &envelope.metadata;

        let event_id = m.event_id.to_string();
        let event_type = m.event_type.to_string();
        let aggregate_type = m.aggregate_type.to_string();
        let aggregate_id = m.aggregate_id.to_string();
        let source_service = m.source_service.to_string();

        let mut headers = OwnedHeaders::new_with_capacity(7)
            .insert(Header {
                key: "event_id",
                value: Some(event_id.as_str()),
            })
            .insert(Header {
                key: "event_type",
                value: Some(event_type.as_str()),
            })
            .insert(Header {
                key: "aggregate_type",
                value: Some(aggregate_type.as_str()),
            })
            .insert(Header {
                key: "aggregate_id",
                value: Some(aggregate_id.as_str()),
            })
            .insert(Header {
                key: "source_service",
                value: Some(source_service.as_str()),
            });

        if let Some(ref cid) = m.correlation_id {
            headers = headers.insert(Header {
                key: "correlation_id",
                value: Some(cid.as_str()),
            });
        }

        if let Some(cause) = m.causation_id {
            let cause_str = cause.to_string();
            headers = headers.insert(Header {
                key: "causation_id",
                value: Some(cause_str.as_str()),
            });
        }

        let record = FutureRecord::to(topic)
            .key(&key)
            .payload(&payload)
            .headers(headers);

        self.producer
            .send(record, Duration::from_secs(5))
            .await
            .map_err(|(e, _)| AppError::InternalServerError(format!("Kafka send: {e}")))?;

        tracing::debug!(
            topic = %topic,
            event_type = %m.event_type,
            aggregate_id = %m.aggregate_id,
            "Published event to Kafka"
        );

        Ok(())
    }
}
