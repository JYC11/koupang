#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub access_token_secret: Vec<u8>,
    pub refresh_token_secret: Vec<u8>,
    pub access_token_expiry_secs: u64,
    pub refresh_token_expiry_secs: u64,
}

impl AuthConfig {
    pub fn new() -> Self {
        let access_token_secret = std::env::var("ACCESS_TOKEN_SECRET")
            .expect("ACCESS_TOKEN_SECRET must be set")
            .as_bytes()
            .to_vec();
        let refresh_token_secret = std::env::var("REFRESH_TOKEN_SECRET")
            .expect("REFRESH_TOKEN_SECRET must be set")
            .as_bytes()
            .to_vec();
        let access_token_expiry_secs: u64 = std::env::var("ACCESS_TOKEN_EXPIRY")
            .expect("ACCESS_TOKEN_EXPIRY must be set")
            .parse()
            .expect("ACCESS_TOKEN_EXPIRY must be a valid u64");
        let refresh_token_expiry_secs: u64 = std::env::var("REFRESH_TOKEN_EXPIRY")
            .expect("REFRESH_TOKEN_EXPIRY must be set")
            .parse()
            .expect("REFRESH_TOKEN_EXPIRY must be a valid u64");

        assert!(
            !access_token_secret.is_empty(),
            "ACCESS_TOKEN_SECRET must not be empty"
        );
        assert!(
            !refresh_token_secret.is_empty(),
            "REFRESH_TOKEN_SECRET must not be empty"
        );
        assert!(
            access_token_expiry_secs > 0,
            "ACCESS_TOKEN_EXPIRY must be positive"
        );
        assert!(
            refresh_token_expiry_secs > 0,
            "REFRESH_TOKEN_EXPIRY must be positive"
        );
        assert!(
            refresh_token_expiry_secs > access_token_expiry_secs,
            "REFRESH_TOKEN_EXPIRY must be greater than ACCESS_TOKEN_EXPIRY"
        );

        Self {
            access_token_secret,
            refresh_token_secret,
            access_token_expiry_secs,
            refresh_token_expiry_secs,
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
