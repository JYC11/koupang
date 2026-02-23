use crate::config::auth_config::AuthConfig;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessTokenClaims {
    pub sub: Uuid,    // Subject (User ID)
    pub name: String, // Custom Claim
    pub role: String, // Custom Claim
    pub iat: i64,     // Issued At
    pub exp: i64,     // Expiration Time
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RefreshTokenClaims {
    pub sub: Uuid,   // Subject (User ID)
    pub jti: String, // JWT ID (Unique identifier for revocation)
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug)]
pub enum AuthError {
    JwtError(jsonwebtoken::errors::Error),
    TimeError(SystemTimeError),
    TokenExpired,
    InvalidToken,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::JwtError(err) => write!(f, "JWT Error: {}", err),
            AuthError::TimeError(err) => write!(f, "Time Error: {}", err),
            AuthError::TokenExpired => write!(f, "Token Expired"),
            AuthError::InvalidToken => write!(f, "Invalid Token"),
        }
    }
}

impl From<jsonwebtoken::errors::Error> for AuthError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        AuthError::JwtError(err)
    }
}

impl From<SystemTimeError> for AuthError {
    fn from(err: SystemTimeError) -> Self {
        AuthError::TimeError(err)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CurrentUser {
    pub id: Uuid,
    pub role: String,
}

impl CurrentUser {
    pub fn can_access(&self, target_user_id: &Uuid) -> bool {
        self.id == *target_user_id || self.role == "ADMIN"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtTokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Clone)]
pub struct JwtService {
    config: AuthConfig,
}

impl JwtService {
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }

    fn current_timestamp() -> Result<i64, AuthError> {
        Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64)
    }

    pub fn generate_access_token(
        &self,
        user_id: &Uuid,
        name: &str,
        role: &str,
    ) -> Result<String, AuthError> {
        let now = Self::current_timestamp()?;

        let claims = AccessTokenClaims {
            sub: user_id.to_owned(),
            name: name.to_owned(),
            role: role.to_owned(),
            iat: now,
            exp: now + self.config.access_token_expiry_secs as i64,
        };

        let header = Header::new(Algorithm::HS256);
        let encoding_key = EncodingKey::from_secret(&self.config.access_token_secret);

        encode(&header, &claims, &encoding_key).map_err(AuthError::from)
    }

    pub fn generate_refresh_token(&self, user_id: &Uuid) -> Result<String, AuthError> {
        let now = Self::current_timestamp()?;

        let jti = Uuid::new_v4().to_string();

        let claims = RefreshTokenClaims {
            sub: user_id.to_owned(),
            jti,
            iat: now,
            exp: now + self.config.refresh_token_expiry_secs as i64,
        };

        let header = Header::new(Algorithm::HS256);
        let encoding_key = EncodingKey::from_secret(&self.config.refresh_token_secret);

        encode(&header, &claims, &encoding_key).map_err(AuthError::from)
    }

    pub fn validate_access_token(&self, token: &str) -> Result<AccessTokenClaims, AuthError> {
        let decoding_key = DecodingKey::from_secret(&self.config.access_token_secret);
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;

        let token_data = match decode::<AccessTokenClaims>(token, &decoding_key, &validation) {
            Ok(token_data) => token_data,
            Err(e) => {
                println!("{:?}", e);
                return Err(AuthError::InvalidToken);
            }
        };

        if token_data.claims.exp < Self::current_timestamp()? {
            return Err(AuthError::TokenExpired);
        };
        Ok(token_data.claims)
    }

    pub fn validate_refresh_token(&self, token: &str) -> Result<RefreshTokenClaims, AuthError> {
        let decoding_key = DecodingKey::from_secret(&self.config.refresh_token_secret);
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;

        let token_data = match decode::<RefreshTokenClaims>(token, &decoding_key, &validation) {
            Ok(token_data) => token_data,
            Err(e) => {
                println!("{:?}", e);
                return Err(AuthError::InvalidToken);
            }
        };

        if token_data.claims.exp < Self::current_timestamp()? {
            return Err(AuthError::TokenExpired);
        };
        Ok(token_data.claims)
    }

    pub fn refresh_access(
        &self,
        refresh_token: &str,
        name: &str,
        role: &str,
    ) -> Result<String, AuthError> {
        let claims = self.validate_refresh_token(refresh_token)?;

        // (Optional) Check against a blacklist database using claims.jti
        // if self.is_blacklisted(&claims.jti) { return Err(AuthError::InvalidToken); }

        let new_access_token = self.generate_access_token(&claims.sub, name, role)?;

        Ok(new_access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    // Helper to create a service with test configuration
    fn get_test_service() -> JwtService {
        let config = AuthConfig::for_tests();
        JwtService::new(config)
    }

    // Helper to create validation with no leeway (for testing expiration)
    fn create_strict_validation(algorithm: Algorithm) -> Validation {
        let mut validation = Validation::new(algorithm);
        validation.validate_exp = true;
        validation.leeway = 0; // No clock skew tolerance for tests
        validation
    }

    #[test]
    fn test_generate_and_validate_access_token() {
        let service = get_test_service();
        let user_id = Uuid::new_v4();
        let name = "Test User";
        let role = "admin";

        let token = service
            .generate_access_token(&user_id, name, role)
            .expect("Failed to generate access token");

        let claims = service
            .validate_access_token(&token)
            .expect("Failed to validate access token");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.name, name);
        assert_eq!(claims.role, role);
    }

    #[test]
    fn test_generate_and_validate_refresh_token() {
        let service = get_test_service();
        let user_id = Uuid::new_v4();

        let token = service
            .generate_refresh_token(&user_id)
            .expect("Failed to generate refresh token");

        let claims = service
            .validate_refresh_token(&token)
            .expect("Failed to validate refresh token");

        assert_eq!(claims.sub, user_id);
        assert!(!claims.jti.is_empty());
    }

    #[test]
    fn test_access_token_invalid_signature() {
        let service = get_test_service();
        let user_id = Uuid::new_v4();

        let token = service
            .generate_access_token(&user_id, "Name", "Role")
            .unwrap();

        let mut config = AuthConfig::for_tests();
        config.access_token_secret = b"different_secret".to_vec();
        let wrong_service = JwtService::new(config);

        let result = wrong_service.validate_access_token(&token);

        assert!(result.is_err());
    }

    #[test]
    fn test_refresh_token_invalid_signature() {
        let service = get_test_service();
        let user_id = Uuid::new_v4();

        let token = service.generate_refresh_token(&user_id).unwrap();

        let mut config = AuthConfig::for_tests();
        config.refresh_token_secret = b"different_secret".to_vec();
        let wrong_service = JwtService::new(config);

        let result = wrong_service.validate_refresh_token(&token);

        assert!(result.is_err());
    }

    #[test]
    fn test_access_token_expiration() {
        let mut config = AuthConfig::for_tests();
        config.access_token_expiry_secs = 1;
        let service = JwtService::new(config);
        let user_id = Uuid::new_v4();
        let token = service
            .generate_access_token(&user_id, "Name", "Role")
            .unwrap();

        // Token should be valid immediately
        assert!(service.validate_access_token(&token).is_ok());

        // Wait for expiration + leeway buffer
        thread::sleep(Duration::from_secs(2));

        // Token should now be invalid
        // Note: In production, leeway is good. For tests, we wait longer.
        let result = service.validate_access_token(&token);
        assert!(result.is_err(), "Expected expired token to be invalid");
    }

    #[test]
    fn test_refresh_token_expiration() {
        let mut config = AuthConfig::for_tests();
        config.refresh_token_expiry_secs = 1;
        let service = JwtService::new(config);
        let user_id = Uuid::new_v4();

        let token = service.generate_refresh_token(&user_id).unwrap();

        assert!(service.validate_refresh_token(&token).is_ok());

        thread::sleep(Duration::from_secs(2));

        assert!(
            service.validate_refresh_token(&token).is_err(),
            "Expected expired refresh token to be invalid"
        );
    }

    #[test]
    fn test_access_token_expiration_strict() {
        // This test uses strict validation with no leeway
        let mut config = AuthConfig::for_tests();
        config.access_token_expiry_secs = 1;
        let service = JwtService::new(config.clone());
        let user_id = Uuid::new_v4();

        let token = service
            .generate_access_token(&user_id, "Name", "Role")
            .unwrap();

        // Validate with strict settings (no leeway)
        let decoding_key = DecodingKey::from_secret(&config.access_token_secret.clone());
        let validation = create_strict_validation(Algorithm::HS256);

        // Should be valid immediately
        assert!(decode::<AccessTokenClaims>(&token, &decoding_key, &validation).is_ok());

        // Wait for expiration
        thread::sleep(Duration::from_secs(2));

        // Should be invalid with strict validation
        let result = decode::<AccessTokenClaims>(&token, &decoding_key, &validation);
        assert!(
            result.is_err(),
            "Expected expired token to be invalid with strict validation"
        );
    }

    #[test]
    fn test_refresh_access_token_flow() {
        let service = get_test_service();
        let user_id = Uuid::new_v4();
        let new_name = "Updated Name";
        let new_role = "super_admin";

        let refresh_token = service
            .generate_refresh_token(&user_id)
            .expect("Failed to generate refresh token");

        let new_access_token = service
            .refresh_access(&refresh_token, new_name, new_role)
            .expect("Failed to refresh access token");

        let claims = service
            .validate_access_token(&new_access_token)
            .expect("Failed to validate new access token");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.name, new_name);
        assert_eq!(claims.role, new_role);

        let now = JwtService::current_timestamp().unwrap();
        assert!(claims.exp > now);
    }

    #[test]
    fn test_refresh_with_invalid_token() {
        let service = get_test_service();
        let user_id = Uuid::new_v4();

        let result = service.refresh_access("invalid.token.here", "Name", "Role");
        assert!(result.is_err());

        let access_token = service
            .generate_access_token(&user_id, "Name", "Role")
            .unwrap();
        let result = service.refresh_access(&access_token, "Name", "Role");
        assert!(result.is_err());
    }
}
