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
        let data: String = match conn.clone().get(key).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(key, error = %e, "cache GET failed");
                return None;
            }
        };
        match serde_json::from_str(&data) {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!(key, error = %e, "cache deserialization failed — possible schema drift");
                None
            }
        }
    }

    pub async fn set<T: serde::Serialize>(&self, key: &str, value: &T) {
        let conn = self.conn.as_ref();
        let Some(conn) = conn else { return };
        let data = match serde_json::to_string(value) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!(key, error = %e, "cache serialization failed");
                return;
            }
        };
        if let Err(e) = conn
            .clone()
            .set_ex::<_, _, ()>(key, &data, self.default_ttl)
            .await
        {
            tracing::warn!(key, error = %e, "cache SET failed");
        }
    }

    pub async fn evict(&self, key: &str) {
        let Some(conn) = self.conn.as_ref() else {
            return;
        };
        if let Err(e) = conn.clone().del::<_, ()>(key).await {
            tracing::warn!(key, error = %e, "cache DEL failed");
        }
    }
}
