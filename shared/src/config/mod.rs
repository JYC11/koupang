pub mod auth_config;
pub mod consumer_config;
pub mod db_config;
pub mod kafka_config;
pub mod redis_config;
pub mod relay_config;

/// Read an environment variable, returning `default` if unset.
pub(crate) fn read_env_or(key: &str, default: String) -> String {
    std::env::var(key).unwrap_or(default)
}

/// Read an environment variable and parse it, returning `default` if unset or unparseable.
pub(crate) fn parse_env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
