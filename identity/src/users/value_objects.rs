use once_cell::sync::Lazy;
use regex::Regex;
use shared::errors::AppError;
use std::fmt;

static EMAIL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap());

static PHONE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\+[1-9]\d{0,2}(-?\d+)+$").unwrap());

static USERNAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap());

shared::valid_id!(UserId);
shared::valid_id!(PasswordTokenId);
shared::valid_id!(EmailTokenId);

#[derive(Debug, Clone)]
pub struct Email(String);

impl Email {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim();

        if trimmed.len() > 254 {
            return Err(AppError::BadRequest(
                "Email must not exceed 254 characters".to_string(),
            ));
        }

        if !EMAIL_RE.is_match(trimmed) {
            return Err(AppError::BadRequest("Invalid email format".to_string()));
        }

        Ok(Self(trimmed.to_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Password ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Password(String);

impl Password {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let mut missing = Vec::new();

        if input.len() < 8 {
            missing.push("at least 8 characters");
        }
        if !input.chars().any(|c| c.is_uppercase()) {
            missing.push("an uppercase letter");
        }
        if !input.chars().any(|c| c.is_lowercase()) {
            missing.push("a lowercase letter");
        }
        if !input.chars().any(|c| c.is_ascii_digit()) {
            missing.push("a digit");
        }
        if !input
            .chars()
            .any(|c| "!@#$%^&*()_+-=[]{}|;':\",./<>?".contains(c))
        {
            missing.push("a special character");
        }

        if !missing.is_empty() {
            return Err(AppError::BadRequest(format!(
                "Password must contain: {}",
                missing.join(", ")
            )));
        }

        Ok(Self(input.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("********")
    }
}

// ── Phone ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Phone(String);

impl Phone {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim();

        if !PHONE_RE.is_match(trimmed) {
            return Err(AppError::BadRequest(
                "Invalid phone format. Expected: +{country code}-{digits} (e.g. +82-10-1234-5678)"
                    .to_string(),
            ));
        }

        let digit_count = trimmed.chars().filter(|c| c.is_ascii_digit()).count();
        if !(7..=15).contains(&digit_count) {
            return Err(AppError::BadRequest(
                "Phone number must contain between 7 and 15 digits".to_string(),
            ));
        }

        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Phone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Username ────────────────────────────────────────────────

// would be nice to add a profanity filter here, but it's not worth the dependency yet
const USERNAME_BLOCKLIST: &[&str] = &["admin", "root", "superuser", "moderator"];

#[derive(Debug, Clone)]
pub struct Username(String);

impl Username {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim();

        if trimmed.len() < 3 {
            return Err(AppError::BadRequest(
                "Username must be at least 3 characters".to_string(),
            ));
        }

        if trimmed.len() > 30 {
            return Err(AppError::BadRequest(
                "Username must not exceed 30 characters".to_string(),
            ));
        }

        if !USERNAME_RE.is_match(trimmed) {
            return Err(AppError::BadRequest(
                "Username may only contain letters, digits, underscores, and hyphens".to_string(),
            ));
        }

        let lower = trimmed.to_lowercase();
        if USERNAME_BLOCKLIST.contains(&lower.as_str()) {
            return Err(AppError::BadRequest(
                "This username is not allowed".to_string(),
            ));
        }

        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Email tests ─────────────────────────────────────────

    #[test]
    fn email_valid() {
        assert!(Email::new("user@example.com").is_ok());
        assert!(Email::new("user.name+tag@domain.co").is_ok());
        assert!(Email::new("u@d.io").is_ok());
    }

    #[test]
    fn email_normalizes_to_lowercase() {
        let email = Email::new("User@Example.COM").unwrap();
        assert_eq!(email.as_str(), "user@example.com");
    }

    #[test]
    fn email_trims_whitespace() {
        let email = Email::new("  user@example.com  ").unwrap();
        assert_eq!(email.as_str(), "user@example.com");
    }

    #[test]
    fn email_rejects_missing_at() {
        assert!(Email::new("userexample.com").is_err());
    }

    #[test]
    fn email_rejects_missing_domain() {
        assert!(Email::new("user@").is_err());
    }

    #[test]
    fn email_rejects_missing_tld() {
        assert!(Email::new("user@domain").is_err());
    }

    #[test]
    fn email_rejects_empty() {
        assert!(Email::new("").is_err());
    }

    #[test]
    fn email_rejects_too_long() {
        let long_local = "a".repeat(250);
        let email = format!("{}@b.co", long_local);
        assert!(Email::new(&email).is_err());
    }

    // ── Password tests ──────────────────────────────────────

    #[test]
    fn password_valid() {
        assert!(Password::new("Password1!").is_ok());
        assert!(Password::new("Str0ng@Pass").is_ok());
        assert!(Password::new("abcDEF12#$").is_ok());
    }

    #[test]
    fn password_rejects_too_short() {
        let err = Password::new("Pa1!").unwrap_err();
        assert!(err.to_string().contains("at least 8 characters"));
    }

    #[test]
    fn password_rejects_no_uppercase() {
        let err = Password::new("password1!").unwrap_err();
        assert!(err.to_string().contains("uppercase"));
    }

    #[test]
    fn password_rejects_no_lowercase() {
        let err = Password::new("PASSWORD1!").unwrap_err();
        assert!(err.to_string().contains("lowercase"));
    }

    #[test]
    fn password_rejects_no_digit() {
        let err = Password::new("Password!!").unwrap_err();
        assert!(err.to_string().contains("digit"));
    }

    #[test]
    fn password_rejects_no_special() {
        let err = Password::new("Password11").unwrap_err();
        assert!(err.to_string().contains("special character"));
    }

    #[test]
    fn password_lists_all_missing_requirements() {
        let err = Password::new("abc").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("at least 8 characters"));
        assert!(msg.contains("uppercase"));
        assert!(msg.contains("digit"));
        assert!(msg.contains("special character"));
    }

    #[test]
    fn password_display_masks_value() {
        let pw = Password::new("Password1!").unwrap();
        assert_eq!(format!("{}", pw), "********");
    }

    // ── Phone tests ─────────────────────────────────────────

    #[test]
    fn phone_valid() {
        assert!(Phone::new("+82-10-1234-5678").is_ok());
        assert!(Phone::new("+1-555-123-4567").is_ok());
        assert!(Phone::new("+441234567890").is_ok());
        assert!(Phone::new("+82101234567").is_ok());
    }

    #[test]
    fn phone_rejects_no_country_code() {
        assert!(Phone::new("010-1234-5678").is_err());
    }

    #[test]
    fn phone_rejects_missing_plus() {
        assert!(Phone::new("82-10-1234-5678").is_err());
    }

    #[test]
    fn phone_rejects_empty() {
        assert!(Phone::new("").is_err());
    }

    #[test]
    fn phone_rejects_letters() {
        assert!(Phone::new("+82-10-abcd-5678").is_err());
    }

    #[test]
    fn phone_rejects_too_few_digits() {
        assert!(Phone::new("+1-123").is_err());
    }

    #[test]
    fn phone_rejects_too_many_digits() {
        assert!(Phone::new("+1-1234567890123456").is_err());
    }

    #[test]
    fn phone_trims_whitespace() {
        let phone = Phone::new("  +82-10-1234-5678  ").unwrap();
        assert_eq!(phone.as_str(), "+82-10-1234-5678");
    }

    // ── Username tests ──────────────────────────────────────

    #[test]
    fn username_valid() {
        assert!(Username::new("testuser").is_ok());
        assert!(Username::new("user_123").is_ok());
        assert!(Username::new("my-name").is_ok());
        assert!(Username::new("abc").is_ok());
    }

    #[test]
    fn username_rejects_too_short() {
        assert!(Username::new("ab").is_err());
    }

    #[test]
    fn username_rejects_too_long() {
        let long = "a".repeat(31);
        assert!(Username::new(&long).is_err());
    }

    #[test]
    fn username_rejects_special_chars() {
        assert!(Username::new("user@name").is_err());
        assert!(Username::new("user name").is_err());
        assert!(Username::new("user.name").is_err());
    }

    #[test]
    fn username_rejects_empty() {
        assert!(Username::new("").is_err());
    }

    #[test]
    fn username_rejects_blocklisted() {
        assert!(Username::new("admin").is_err());
        assert!(Username::new("Admin").is_err());
        assert!(Username::new("ROOT").is_err());
        assert!(Username::new("superuser").is_err());
        assert!(Username::new("moderator").is_err());
    }

    #[test]
    fn username_trims_whitespace() {
        let u = Username::new("  testuser  ").unwrap();
        assert_eq!(u.as_str(), "testuser");
    }

    #[test]
    fn username_boundary_30_chars() {
        let exactly_30 = "a".repeat(30);
        assert!(Username::new(&exactly_30).is_ok());
    }

    #[test]
    fn username_boundary_3_chars() {
        assert!(Username::new("abc").is_ok());
    }
}
