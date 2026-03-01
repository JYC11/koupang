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
