#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub access_token_secret: Vec<u8>,
    pub refresh_token_secret: Vec<u8>,
    pub access_token_expiry_secs: u64,
    pub refresh_token_expiry_secs: u64,
}

impl AuthConfig {
    pub fn new() -> Self {
        Self {
            access_token_secret: std::env::var("ACCESS_TOKEN_SECRET").unwrap().as_bytes().to_vec(),
            refresh_token_secret: std::env::var("REFRESH_TOKEN_SECRET").unwrap().as_bytes().to_vec(),
            access_token_expiry_secs: std::env::var("ACCESS_TOKEN_EXPIRY").unwrap().parse().unwrap(),
            refresh_token_expiry_secs: std::env::var("REFRESH_TOKEN_EXPIRY").unwrap().parse().unwrap(),
        }
    }

    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            access_token_secret: b"test access secret".to_vec(),
            refresh_token_secret: b"test refresh secret".to_vec(),
            access_token_expiry_secs: 1,
            refresh_token_expiry_secs: 1,
        }
    }
}
