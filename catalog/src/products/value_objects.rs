use once_cell::sync::Lazy;
use regex::Regex;
use rust_decimal::Decimal;
use shared::errors::AppError;
use std::fmt;

// Re-export shared value objects so existing imports still work
pub use crate::common::value_objects::{HttpUrl, Slug};

// Type alias for backwards compatibility
pub type ImageUrl = HttpUrl;

// ── Regexes (compiled once) ─────────────────────────────────

static SKU_CODE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,98}[A-Za-z0-9]$").unwrap());

// ── ProductName (via macro) ───────────────────────────────

crate::validated_name!(ProductName, "Product name", 500);

// ── Price ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Price(Decimal);

impl Price {
    pub fn new(value: Decimal) -> Result<Self, AppError> {
        if value < Decimal::ZERO {
            return Err(AppError::BadRequest(
                "Price must not be negative".to_string(),
            ));
        }

        Ok(Self(value))
    }

    pub fn value(&self) -> Decimal {
        self.0
    }

    pub fn into_inner(self) -> Decimal {
        self.0
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── SkuCode ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SkuCode(String);

impl SkuCode {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim();

        if trimmed.len() < 2 {
            return Err(AppError::BadRequest(
                "SKU code must be at least 2 characters".to_string(),
            ));
        }

        if trimmed.len() > 100 {
            return Err(AppError::BadRequest(
                "SKU code must not exceed 100 characters".to_string(),
            ));
        }

        if !SKU_CODE_RE.is_match(trimmed) {
            return Err(AppError::BadRequest(
                "SKU code must be alphanumeric (with hyphens/underscores allowed in the middle)"
                    .to_string(),
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

impl fmt::Display for SkuCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── StockQuantity ───────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct StockQuantity(i32);

impl StockQuantity {
    pub fn new(value: i32) -> Result<Self, AppError> {
        if value < 0 {
            return Err(AppError::BadRequest(
                "Stock quantity must not be negative".to_string(),
            ));
        }

        Ok(Self(value))
    }

    pub fn value(&self) -> i32 {
        self.0
    }
}

impl fmt::Display for StockQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Currency ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Currency(String);

impl Currency {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim().to_uppercase();

        if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(AppError::BadRequest(
                "Currency must be a 3-letter ISO 4217 code (e.g. USD, KRW)".to_string(),
            ));
        }

        Ok(Self(trimmed))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Default for Currency {
    fn default() -> Self {
        Self("USD".to_string())
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── ProductStatus ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum ProductStatus {
    Draft,
    Active,
    Inactive,
    Archived,
}

impl ProductStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProductStatus::Draft => "draft",
            ProductStatus::Active => "active",
            ProductStatus::Inactive => "inactive",
            ProductStatus::Archived => "archived",
        }
    }
}

impl fmt::Display for ProductStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── SkuStatus ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum SkuStatus {
    Active,
    Inactive,
    OutOfStock,
}

impl SkuStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkuStatus::Active => "active",
            SkuStatus::Inactive => "inactive",
            SkuStatus::OutOfStock => "out_of_stock",
        }
    }
}

impl fmt::Display for SkuStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ProductName tests ────────────────────────────────────

    #[test]
    fn product_name_valid() {
        assert!(ProductName::new("Widget Pro").is_ok());
        assert!(ProductName::new("A").is_ok());
    }

    #[test]
    fn product_name_trims_whitespace() {
        let name = ProductName::new("  Widget Pro  ").unwrap();
        assert_eq!(name.as_str(), "Widget Pro");
    }

    #[test]
    fn product_name_rejects_empty() {
        assert!(ProductName::new("").is_err());
        assert!(ProductName::new("   ").is_err());
    }

    #[test]
    fn product_name_rejects_too_long() {
        let long = "a".repeat(501);
        assert!(ProductName::new(&long).is_err());
    }

    // ── Slug tests (from common, verify re-export) ──────────

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

    // ── Price tests ──────────────────────────────────────────

    #[test]
    fn price_valid() {
        assert!(Price::new(Decimal::new(1999, 2)).is_ok()); // 19.99
        assert!(Price::new(Decimal::ZERO).is_ok());
    }

    #[test]
    fn price_rejects_negative() {
        assert!(Price::new(Decimal::new(-1, 0)).is_err());
    }

    // ── SkuCode tests ────────────────────────────────────────

    #[test]
    fn sku_code_valid() {
        assert!(SkuCode::new("SKU-001").is_ok());
        assert!(SkuCode::new("WIDGET_BLUE_XL").is_ok());
        assert!(SkuCode::new("AB").is_ok());
    }

    #[test]
    fn sku_code_rejects_too_short() {
        assert!(SkuCode::new("A").is_err());
    }

    #[test]
    fn sku_code_rejects_too_long() {
        let long = "A".repeat(101);
        assert!(SkuCode::new(&long).is_err());
    }

    #[test]
    fn sku_code_rejects_leading_hyphen() {
        assert!(SkuCode::new("-SKU").is_err());
    }

    // ── StockQuantity tests ──────────────────────────────────

    #[test]
    fn stock_valid() {
        assert!(StockQuantity::new(0).is_ok());
        assert!(StockQuantity::new(1000).is_ok());
    }

    #[test]
    fn stock_rejects_negative() {
        assert!(StockQuantity::new(-1).is_err());
    }

    // ── Currency tests ───────────────────────────────────────

    #[test]
    fn currency_valid() {
        assert!(Currency::new("USD").is_ok());
        assert!(Currency::new("krw").is_ok()); // normalizes to uppercase
    }

    #[test]
    fn currency_normalizes_to_uppercase() {
        let c = Currency::new("usd").unwrap();
        assert_eq!(c.as_str(), "USD");
    }

    #[test]
    fn currency_rejects_wrong_length() {
        assert!(Currency::new("US").is_err());
        assert!(Currency::new("USDD").is_err());
    }

    #[test]
    fn currency_rejects_digits() {
        assert!(Currency::new("US1").is_err());
    }

    #[test]
    fn currency_default_is_usd() {
        assert_eq!(Currency::default().as_str(), "USD");
    }

    // ── ImageUrl tests (alias for HttpUrl) ──────────────────

    #[test]
    fn image_url_valid() {
        assert!(ImageUrl::new("https://example.com/img.jpg").is_ok());
        assert!(ImageUrl::new("http://cdn.example.com/a.png").is_ok());
    }

    #[test]
    fn image_url_rejects_no_scheme() {
        assert!(ImageUrl::new("example.com/img.jpg").is_err());
    }

    #[test]
    fn image_url_rejects_empty() {
        assert!(ImageUrl::new("").is_err());
    }

    // ── ProductStatus tests ──────────────────────────────────

    #[test]
    fn product_status_as_str() {
        assert_eq!(ProductStatus::Draft.as_str(), "draft");
        assert_eq!(ProductStatus::Active.as_str(), "active");
        assert_eq!(ProductStatus::Inactive.as_str(), "inactive");
        assert_eq!(ProductStatus::Archived.as_str(), "archived");
    }

    // ── SkuStatus tests ─────────────────────────────────────

    #[test]
    fn sku_status_as_str() {
        assert_eq!(SkuStatus::Active.as_str(), "active");
        assert_eq!(SkuStatus::Inactive.as_str(), "inactive");
        assert_eq!(SkuStatus::OutOfStock.as_str(), "out_of_stock");
    }
}
