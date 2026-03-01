pub struct KafkaConfig {
    pub brokers: String,
}

impl KafkaConfig {
    /// Reads `KAFKA_BROKERS` from environment, defaults to `"localhost:29092"`.
    pub fn new() -> Self {
        Self {
            brokers: std::env::var("KAFKA_BROKERS")
                .unwrap_or_else(|_| "localhost:29092".to_string()),
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
