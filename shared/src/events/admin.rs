use std::collections::HashMap;
use std::time::Duration;

use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::config::ClientConfig;
use rdkafka::error::RDKafkaErrorCode;

use crate::config::kafka_config::KafkaConfig;
use crate::errors::AppError;

/// Describes a Kafka topic to be created.
pub struct TopicSpec {
    pub name: String,
    pub partitions: i32,
    pub replication_factor: i32,
    pub config: HashMap<String, String>,
}

impl TopicSpec {
    pub fn new(name: impl Into<String>, partitions: i32, replication_factor: i32) -> Self {
        Self {
            name: name.into(),
            partitions,
            replication_factor,
            config: HashMap::new(),
        }
    }

    /// Add a topic-level config entry (e.g. `"retention.ms"`, `"86400000"`).
    pub fn with_config(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.insert(key.into(), value.into());
        self
    }
}

/// Thin wrapper around rdkafka's `AdminClient` for idempotent topic management.
pub struct KafkaAdmin {
    admin: AdminClient<DefaultClientContext>,
}

impl KafkaAdmin {
    pub fn new(config: &KafkaConfig) -> Result<Self, AppError> {
        let admin: AdminClient<DefaultClientContext> = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .create()
            .map_err(|e| AppError::InternalServerError(format!("Kafka admin init: {e}")))?;

        Ok(Self { admin })
    }

    /// Creates topics idempotently — existing topics are silently skipped.
    pub async fn ensure_topics(&self, specs: &[TopicSpec]) -> Result<(), AppError> {
        if specs.is_empty() {
            return Ok(());
        }

        // Build NewTopic descriptors with borrowed config entries.
        // We must keep the owned Vec<(&str, &str)> vecs alive for the borrow to be valid.
        let config_refs: Vec<Vec<(&str, &str)>> = specs
            .iter()
            .map(|s| {
                s.config
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect()
            })
            .collect();

        let new_topics: Vec<NewTopic<'_>> = specs
            .iter()
            .zip(config_refs.iter())
            .map(|(spec, cfg)| {
                let mut topic = NewTopic::new(
                    &spec.name,
                    spec.partitions,
                    TopicReplication::Fixed(spec.replication_factor),
                );
                for &(k, v) in cfg {
                    topic = topic.set(k, v);
                }
                topic
            })
            .collect();

        let opts = AdminOptions::new().request_timeout(Some(Duration::from_secs(10)));
        let results = self
            .admin
            .create_topics(&new_topics, &opts)
            .await
            .map_err(|e| AppError::InternalServerError(format!("Kafka create_topics: {e}")))?;

        for result in results {
            match result {
                Ok(_) => {}
                Err((_, RDKafkaErrorCode::TopicAlreadyExists)) => {}
                Err((topic, code)) => {
                    return Err(AppError::InternalServerError(format!(
                        "Failed to create topic '{topic}': {code:?}"
                    )));
                }
            }
        }

        Ok(())
    }
}
