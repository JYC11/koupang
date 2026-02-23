use crate::config::redis_config::RedisConfig;

pub async fn init_redis(config: RedisConfig) -> redis::aio::ConnectionManager {
    let client = redis::Client::open(config.url.as_str()).expect("Failed to create Redis client");
    redis::aio::ConnectionManager::new(client)
        .await
        .expect("Failed to create Redis connection manager")
}

/// Initializes Redis if `REDIS_URL` is set. Returns `None` otherwise.
pub async fn init_optional_redis() -> Option<redis::aio::ConnectionManager> {
    match RedisConfig::try_new() {
        Some(config) => {
            let conn = init_redis(config).await;
            tracing::info!("Redis connection established");
            Some(conn)
        }
        None => {
            tracing::info!("REDIS_URL not set, running without cache");
            None
        }
    }
}
