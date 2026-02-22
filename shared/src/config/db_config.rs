pub struct DbConfig {
    pub url: String,
    pub max_connections: u32,
}

impl DbConfig {
    pub fn new(db_url_key: &str) -> Self {
        Self {
            url: std::env::var(db_url_key).unwrap(),
            max_connections: std::env::var("DB_MAX_CONNECTIONS")
                .unwrap()
                .parse()
                .unwrap(),
        }
    }
}
