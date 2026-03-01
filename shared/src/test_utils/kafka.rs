use testcontainers_modules::kafka::apache::{KAFKA_PORT, Kafka};
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use tokio::sync::OnceCell;

use crate::config::kafka_config::KafkaConfig;

/// Shared Kafka container, initialized once per test binary.
struct SharedKafkaContainer {
    _container: ContainerAsync<Kafka>,
    bootstrap_servers: String,
}

static SHARED_KAFKA: OnceCell<SharedKafkaContainer> = OnceCell::const_new();

impl SharedKafkaContainer {
    async fn init() -> Self {
        let container = Kafka::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(KAFKA_PORT).await.unwrap();
        let bootstrap_servers = format!("{host}:{port}");
        Self {
            _container: container,
            bootstrap_servers,
        }
    }
}

pub struct TestKafka {
    pub bootstrap_servers: String,
}

impl TestKafka {
    /// Returns a connection to a shared Kafka container (KRaft, no Zookeeper).
    ///
    /// The first call starts the container. Subsequent calls reuse it.
    /// Topic isolation is achieved via unique topic names (`test-{uuid}`), no cleanup needed.
    pub async fn start() -> Self {
        let shared = SHARED_KAFKA
            .get_or_init(|| SharedKafkaContainer::init())
            .await;
        Self {
            bootstrap_servers: shared.bootstrap_servers.clone(),
        }
    }

    /// Convenience: build a `KafkaConfig` pointing at the test container.
    pub fn kafka_config(&self) -> KafkaConfig {
        KafkaConfig::from_brokers(&self.bootstrap_servers)
    }
}

// ── Test consumer ────────────────────────────────────────────────────

use std::collections::HashMap;
use std::time::Duration;

use rdkafka::Message;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Headers;
use tokio_stream::StreamExt;
use uuid::Uuid;

/// A consumed Kafka message with all fields as owned data.
#[derive(Debug, Clone)]
pub struct ReceivedMessage {
    pub key: String,
    pub payload: String,
    pub headers: HashMap<String, String>,
}

impl ReceivedMessage {
    /// Deserialize the payload as an `EventEnvelope`.
    pub fn envelope(&self) -> crate::events::EventEnvelope {
        serde_json::from_str(&self.payload).expect("payload is a valid EventEnvelope")
    }
}

/// Kafka consumer for integration tests.
///
/// Creates a unique consumer group per instance and subscribes to a single topic.
/// Handles transient `BrokerTransportFailure` errors during startup.
pub struct TestConsumer {
    consumer: StreamConsumer,
}

impl TestConsumer {
    pub fn new(bootstrap_servers: &str, topic: &str) -> Self {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("group.id", &format!("test-group-{}", Uuid::now_v7()))
            .set("auto.offset.reset", "earliest")
            .create()
            .expect("test consumer creation");
        consumer.subscribe(&[topic]).expect("subscribe");
        Self { consumer }
    }

    /// Consume the next message, retrying on transient broker transport errors.
    /// Panics if no message arrives within 30 seconds.
    pub async fn recv(&self) -> ReceivedMessage {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline - tokio::time::Instant::now();
            let result = tokio::time::timeout(remaining, self.consumer.stream().next()).await;
            match result {
                Ok(Some(Ok(msg))) => {
                    let key = msg
                        .key()
                        .map(|k| std::str::from_utf8(k).unwrap().to_string())
                        .unwrap_or_default();
                    let payload = msg
                        .payload()
                        .map(|p| std::str::from_utf8(p).unwrap().to_string())
                        .unwrap_or_default();
                    let mut headers = HashMap::new();
                    if let Some(h) = msg.headers() {
                        for i in 0..h.count() {
                            if let Some(header) = h.try_get(i) {
                                let val = header
                                    .value
                                    .map(|v| std::str::from_utf8(v).unwrap().to_string())
                                    .unwrap_or_default();
                                headers.insert(header.key.to_string(), val);
                            }
                        }
                    }
                    return ReceivedMessage {
                        key,
                        payload,
                        headers,
                    };
                }
                Ok(Some(Err(_))) => {
                    // Transient error (e.g. BrokerTransportFailure) — retry
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
                Ok(None) => panic!("consumer stream ended unexpectedly"),
                Err(_) => panic!("timed out waiting for message (30s)"),
            }
        }
    }
}
