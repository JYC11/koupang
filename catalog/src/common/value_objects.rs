use once_cell::sync::Lazy;
use regex::Regex;
use shared::errors::AppError;
use std::fmt;

// ── Regexes (compiled once) ─────────────────────────────────

static SLUG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*$").unwrap());

static URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^https?://.+").unwrap());

// ── Slug ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Slug(String);

impl Slug {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim();

        if trimmed.is_empty() {
            return Err(AppError::BadRequest("Slug must not be empty".to_string()));
        }

        if trimmed.len() > 500 {
            return Err(AppError::BadRequest(
                "Slug must not exceed 500 characters".to_string(),
            ));
        }

        if !SLUG_RE.is_match(trimmed) {
            return Err(AppError::BadRequest(
                "Slug must be lowercase alphanumeric with hyphens (e.g. my-product-name)"
                    .to_string(),
            ));
        }

        Ok(Self(trimmed.to_string()))
    }

    /// Generate a slug from a human-readable name by lowercasing and replacing non-alphanumeric with hyphens.
    pub fn from_name(name: &str) -> Result<Self, AppError> {
        let slug: String = name
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();

        let slug = slug
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        Self::new(&slug)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── HttpUrl ────────────────────────────────────────────────
//
// Generic validated HTTP/HTTPS URL. Used for image URLs, logo URLs, etc.

#[derive(Debug, Clone)]
pub struct HttpUrl(String);

impl HttpUrl {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim();

        if !URL_RE.is_match(trimmed) {
            return Err(AppError::BadRequest(
                "URL must start with http:// or https://".to_string(),
            ));
        }

        if trimmed.len() > 2048 {
            return Err(AppError::BadRequest(
                "URL must not exceed 2048 characters".to_string(),
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

impl fmt::Display for HttpUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Slug tests ────────────────────────────────────────

    #[test]
    fn slug_valid() {
        assert!(Slug::new("my-product").is_ok());
        assert!(Slug::new("widget123").is_ok());
        assert!(Slug::new("a").is_ok());
    }

    #[test]
    fn slug_rejects_uppercase() {
        assert!(Slug::new("My-Product").is_err());
    }

    #[test]
    fn slug_rejects_spaces() {
        assert!(Slug::new("my product").is_err());
    }

    #[test]
    fn slug_rejects_empty() {
        assert!(Slug::new("").is_err());
    }

    #[test]
    fn slug_from_name() {
        let slug = Slug::from_name("My Awesome Product!").unwrap();
        assert_eq!(slug.as_str(), "my-awesome-product");
    }

    #[test]
    fn slug_from_name_collapses_hyphens() {
        let slug = Slug::from_name("Hello   World").unwrap();
        assert_eq!(slug.as_str(), "hello-world");
    }

    // ── HttpUrl tests ─────────────────────────────────────

    #[test]
    fn http_url_valid() {
        assert!(HttpUrl::new("https://example.com/img.jpg").is_ok());
        assert!(HttpUrl::new("http://cdn.example.com/a.png").is_ok());
    }

    #[test]
    fn http_url_rejects_no_scheme() {
        assert!(HttpUrl::new("example.com/img.jpg").is_err());
    }

    #[test]
    fn http_url_rejects_empty() {
        assert!(HttpUrl::new("").is_err());
    }

    #[test]
    fn http_url_rejects_too_long() {
        let long = format!("https://example.com/{}", "a".repeat(2040));
        assert!(HttpUrl::new(&long).is_err());
    }
}
