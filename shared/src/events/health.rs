use std::sync::Arc;
use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use serde::Serialize;

use crate::config::kafka_config::KafkaConfig;
use crate::errors::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KafkaHealthStatus {
    Up,
    Down,
}

#[derive(Debug, Clone, Serialize)]
pub struct KafkaHealth {
    pub status: KafkaHealthStatus,
    pub broker_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl KafkaHealth {
    pub fn is_healthy(&self) -> bool {
        self.status == KafkaHealthStatus::Up
    }
}

/// Checks Kafka broker connectivity via metadata fetch.
///
/// Uses a lightweight `BaseConsumer` (no consumer group) and wraps the
/// blocking `fetch_metadata` call in `spawn_blocking`.
pub struct KafkaHealthChecker {
    consumer: Arc<BaseConsumer>,
    timeout: Duration,
}

impl KafkaHealthChecker {
    pub fn new(config: &KafkaConfig) -> Result<Self, AppError> {
        let consumer: BaseConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .create()
            .map_err(|e| AppError::InternalServerError(format!("Kafka health client init: {e}")))?;

        Ok(Self {
            consumer: Arc::new(consumer),
            timeout: Duration::from_secs(5),
        })
    }

    pub async fn check(&self) -> KafkaHealth {
        let consumer = Arc::clone(&self.consumer);
        let timeout = self.timeout;

        match tokio::task::spawn_blocking(move || consumer.fetch_metadata(None, timeout)).await {
            Ok(Ok(metadata)) => KafkaHealth {
                status: KafkaHealthStatus::Up,
                broker_count: metadata.brokers().len(),
                error: None,
            },
            Ok(Err(e)) => KafkaHealth {
                status: KafkaHealthStatus::Down,
                broker_count: 0,
                error: Some(e.to_string()),
            },
            Err(e) => KafkaHealth {
                status: KafkaHealthStatus::Down,
                broker_count: 0,
                error: Some(format!("Health check task failed: {e}")),
            },
        }
    }
}
