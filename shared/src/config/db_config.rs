pub struct DbConfig {
    pub url: String,
    pub max_connections: u32,
}

impl DbConfig {
    pub fn new() -> Self {
        Self {
            url: std::env::var("DB_URL").unwrap(),
            max_connections: std::env::var("DB_MAX_CONNECTIONS")
                .unwrap()
                .parse()
                .unwrap(),
        }
    }
}
