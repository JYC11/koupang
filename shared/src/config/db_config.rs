pub struct DbConfig {
    pub url: String,
    pub max_connections: u32,
}

impl DbConfig {
    pub fn new(db_url_key: &str) -> Self {
        Self {
            url: std::env::var(db_url_key)
                .unwrap_or_else(|_| panic!("{db_url_key} env var must be set")),
            max_connections: std::env::var("DB_MAX_CONNECTIONS")
                .expect("DB_MAX_CONNECTIONS env var must be set")
                .parse()
                .expect("DB_MAX_CONNECTIONS must be a valid u32"),
        }
    }
}
