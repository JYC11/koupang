pub struct DbConfig {
    pub url: String,
    pub max_connections: u32,
}

impl DbConfig {
    pub fn new(db_url_key: &str) -> Self {
        let url = std::env::var(db_url_key)
            .unwrap_or_else(|_| panic!("{db_url_key} env var must be set"));
        let max_connections: u32 = std::env::var("DB_MAX_CONNECTIONS")
            .expect("DB_MAX_CONNECTIONS env var must be set")
            .parse()
            .expect("DB_MAX_CONNECTIONS must be a valid u32");

        assert!(!url.is_empty(), "{db_url_key} must not be empty");
        assert!(
            max_connections > 0,
            "DB_MAX_CONNECTIONS must be positive, got {max_connections}"
        );

        Self {
            url,
            max_connections,
        }
    }
}
