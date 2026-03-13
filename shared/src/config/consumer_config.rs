use std::time::Duration;

use super::env_parse;

/// Configuration for `KafkaEventConsumer`. No `Default` — `group_id` and `topics` are required.
pub struct ConsumerConfig {
    /// Kafka consumer group (e.g. `"order-consumer"`).
    pub group_id: String,
    /// Topics to subscribe to.
    pub topics: Vec<String>,
    /// Maximum retry attempts for transient handler errors (default 3).
    pub max_retries: u32,
    /// Base delay for exponential backoff (default 1s). Delays: base, 2×base, 4×base, …
    pub retry_base_delay: Duration,
    /// Cap on backoff delay (default 30s).
    pub retry_max_delay: Duration,
    /// Override the per-topic DLQ naming (`{topic}.dlq`) with a single topic.
    pub dlq_topic_override: Option<String>,
    /// Kafka session timeout (default 30s).
    pub session_timeout: Duration,
    /// Auto-create DLQ topics on startup (default true).
    pub auto_create_dlq_topics: bool,
    /// How often to run `cleanup_processed_events` (default 1 hour).
    pub processed_events_cleanup_interval: Duration,
    /// Max age of processed-event rows before cleanup (default 7 days).
    pub processed_events_max_age: Duration,
}

impl ConsumerConfig {
    /// Construct with required fields; optional fields get sensible defaults.
    pub fn new(group_id: impl Into<String>, topics: Vec<String>) -> Self {
        Self {
            group_id: group_id.into(),
            topics,
            max_retries: 3,
            retry_base_delay: Duration::from_secs(1),
            retry_max_delay: Duration::from_secs(30),
            dlq_topic_override: None,
            session_timeout: Duration::from_secs(30),
            auto_create_dlq_topics: true,
            processed_events_cleanup_interval: Duration::from_secs(3600),
            processed_events_max_age: Duration::from_secs(7 * 24 * 3600),
        }
    }

    /// Build from environment variables, falling back to defaults for optional fields.
    ///
    /// | Variable | Type | Default |
    /// |----------|------|---------|
    /// | `EVENT_CONSUMER_MAX_RETRIES` | u32 | 3 |
    /// | `EVENT_CONSUMER_RETRY_BASE_DELAY_MS` | u64 | 1000 |
    /// | `EVENT_CONSUMER_RETRY_MAX_DELAY_SECS` | u64 | 30 |
    /// | `EVENT_CONSUMER_DLQ_TOPIC` | String | None (`{topic}.dlq`) |
    /// | `EVENT_CONSUMER_SESSION_TIMEOUT_SECS` | u64 | 30 |
    /// | `EVENT_CONSUMER_AUTO_CREATE_DLQ` | bool | true |
    /// | `EVENT_CONSUMER_CLEANUP_INTERVAL_SECS` | u64 | 3600 |
    /// | `EVENT_CONSUMER_CLEANUP_MAX_AGE_SECS` | u64 | 604800 |
    pub fn from_env(group_id: impl Into<String>, topics: Vec<String>) -> Self {
        Self {
            group_id: group_id.into(),
            topics,
            max_retries: env_parse("EVENT_CONSUMER_MAX_RETRIES", 3),
            retry_base_delay: Duration::from_millis(env_parse(
                "EVENT_CONSUMER_RETRY_BASE_DELAY_MS",
                1000,
            )),
            retry_max_delay: Duration::from_secs(env_parse(
                "EVENT_CONSUMER_RETRY_MAX_DELAY_SECS",
                30,
            )),
            dlq_topic_override: std::env::var("EVENT_CONSUMER_DLQ_TOPIC").ok(),
            session_timeout: Duration::from_secs(env_parse(
                "EVENT_CONSUMER_SESSION_TIMEOUT_SECS",
                30,
            )),
            auto_create_dlq_topics: env_parse("EVENT_CONSUMER_AUTO_CREATE_DLQ", true),
            processed_events_cleanup_interval: Duration::from_secs(env_parse(
                "EVENT_CONSUMER_CLEANUP_INTERVAL_SECS",
                3600,
            )),
            processed_events_max_age: Duration::from_secs(env_parse(
                "EVENT_CONSUMER_CLEANUP_MAX_AGE_SECS",
                7 * 24 * 3600,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_config_defaults() {
        let config = ConsumerConfig::new("test-group", vec!["topic.a".into()]);

        assert_eq!(config.group_id, "test-group");
        assert_eq!(config.topics, vec!["topic.a"]);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_base_delay, Duration::from_secs(1));
        assert_eq!(config.retry_max_delay, Duration::from_secs(30));
        assert!(config.dlq_topic_override.is_none());
        assert_eq!(config.session_timeout, Duration::from_secs(30));
        assert!(config.auto_create_dlq_topics);
        assert_eq!(
            config.processed_events_cleanup_interval,
            Duration::from_secs(3600)
        );
        assert_eq!(
            config.processed_events_max_age,
            Duration::from_secs(7 * 24 * 3600)
        );
    }

    #[test]
    fn consumer_config_from_env_reads_overrides() {
        // SAFETY: test-only, single-threaded via --test-threads=1
        unsafe {
            std::env::set_var("EVENT_CONSUMER_MAX_RETRIES", "5");
            std::env::set_var("EVENT_CONSUMER_RETRY_BASE_DELAY_MS", "500");
            std::env::set_var("EVENT_CONSUMER_AUTO_CREATE_DLQ", "false");
            std::env::set_var("EVENT_CONSUMER_DLQ_TOPIC", "custom.dlq");
        }

        let config = ConsumerConfig::from_env("env-group", vec!["t1".into()]);

        assert_eq!(config.group_id, "env-group");
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.retry_base_delay, Duration::from_millis(500));
        assert!(!config.auto_create_dlq_topics);
        assert_eq!(config.dlq_topic_override.as_deref(), Some("custom.dlq"));

        // Non-overridden defaults
        assert_eq!(config.retry_max_delay, Duration::from_secs(30));
        assert_eq!(config.session_timeout, Duration::from_secs(30));

        unsafe {
            std::env::remove_var("EVENT_CONSUMER_MAX_RETRIES");
            std::env::remove_var("EVENT_CONSUMER_RETRY_BASE_DELAY_MS");
            std::env::remove_var("EVENT_CONSUMER_AUTO_CREATE_DLQ");
            std::env::remove_var("EVENT_CONSUMER_DLQ_TOPIC");
        }
    }
}
