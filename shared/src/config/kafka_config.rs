use super::read_env_or;

#[derive(Clone)]
pub struct KafkaConfig {
    pub brokers: String,
}

impl KafkaConfig {
    /// Reads `KAFKA_BROKERS` from environment, defaults to `"localhost:29092"`.
    pub fn new() -> Self {
        Self {
            brokers: read_env_or("KAFKA_BROKERS", "localhost:29092".to_string()),
        }
    }

    /// Explicit broker list, primarily for tests.
    pub fn from_brokers(brokers: impl Into<String>) -> Self {
        Self {
            brokers: brokers.into(),
        }
    }
}

impl Default for KafkaConfig {
    fn default() -> Self {
        Self::new()
    }
}
