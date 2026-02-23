pub struct RedisConfig {
    pub url: String,
}

impl RedisConfig {
    pub fn new() -> Self {
        let url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        Self { url }
    }

    /// Returns None if REDIS_URL is not set (optional in dev/test)
    pub fn try_new() -> Option<Self> {
        std::env::var("REDIS_URL").ok().map(|url| Self { url })
    }
}
