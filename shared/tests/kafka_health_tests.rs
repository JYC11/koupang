use shared::config::kafka_config::KafkaConfig;
use shared::events::{KafkaHealthChecker, KafkaHealthStatus};
use shared::test_utils::kafka::TestKafka;

#[tokio::test]
async fn health_check_reports_up_with_running_broker() {
    let kafka = TestKafka::start().await;
    let checker = KafkaHealthChecker::new(&kafka.kafka_config()).unwrap();

    let health = checker.check().await;

    assert_eq!(health.status, KafkaHealthStatus::Up);
    assert!(health.broker_count >= 1);
    assert!(health.error.is_none());
    assert!(health.is_healthy());
}

#[tokio::test]
async fn health_check_reports_down_with_bad_broker() {
    let config = KafkaConfig::from_brokers("localhost:19999");
    let checker = KafkaHealthChecker::new(&config).unwrap();

    let health = checker.check().await;

    assert_eq!(health.status, KafkaHealthStatus::Down);
    assert_eq!(health.broker_count, 0);
    assert!(health.error.is_some());
    assert!(!health.is_healthy());
}
