use crate::config::redis_config::RedisConfig;
use redis::AsyncCommands;

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

/// Thin wrapper around an optional Redis connection for JSON-based caching.
///
/// All operations silently no-op when Redis is unavailable (graceful degradation).
/// Errors are swallowed — cache is a performance optimization, not a correctness requirement.
#[derive(Clone)]
pub struct RedisCache {
    conn: Option<redis::aio::ConnectionManager>,
    default_ttl: u64,
}

impl RedisCache {
    pub fn new(conn: Option<redis::aio::ConnectionManager>, default_ttl_secs: u64) -> Self {
        Self {
            conn,
            default_ttl: default_ttl_secs,
        }
    }

    /// No-op cache (always misses, never stores). Useful for tests.
    pub fn noop() -> Self {
        Self {
            conn: None,
            default_ttl: 0,
        }
    }

    pub async fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let conn = self.conn.as_ref()?;
        let data: String = conn.clone().get(key).await.ok()?;
        serde_json::from_str(&data).ok()
    }

    pub async fn set<T: serde::Serialize>(&self, key: &str, value: &T) {
        let Some(ref conn) = self.conn else { return };
        let Ok(data) = serde_json::to_string(value) else {
            return;
        };
        let _: Result<(), _> = conn.clone().set_ex(key, &data, self.default_ttl).await;
    }

    pub async fn evict(&self, key: &str) {
        let Some(ref conn) = self.conn else { return };
        let _: Result<(), _> = conn.clone().del(key).await;
    }
}
