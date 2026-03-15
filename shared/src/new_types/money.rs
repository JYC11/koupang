use crate::errors::AppError;
use rust_decimal::Decimal;
use std::fmt;

// ── Price (non-negative Decimal, shared across services) ──

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

// ── Currency (3-letter ISO 4217, shared across services) ──

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

// ── Money (Price + Currency pair) ─────────────────────────

#[derive(Debug, Clone)]
pub struct Money {
    pub price: Price,
    pub currency: Currency,
}

impl Money {
    pub fn new(price: Price, currency: Currency) -> Self {
        Self { price, currency }
    }

    pub fn same_currency(&self, other: &Money) -> bool {
        self.currency == other.currency
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.price, self.currency)
    }
}

// ── Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Price ─────────────────────────────────────────────

    #[test]
    fn price_valid() {
        assert!(Price::new(Decimal::ZERO).is_ok());
        assert!(Price::new(Decimal::new(1999, 2)).is_ok());
    }

    #[test]
    fn price_rejects_negative() {
        assert!(Price::new(Decimal::new(-1, 0)).is_err());
    }

    #[test]
    fn price_display() {
        let p = Price::new(Decimal::new(1999, 2)).unwrap();
        assert_eq!(p.to_string(), "19.99");
    }

    // ── Currency ──────────────────────────────────────────

    #[test]
    fn currency_valid() {
        assert!(Currency::new("USD").is_ok());
        assert!(Currency::new("krw").is_ok());
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

    // ── Money ────────────────────────────────────────────

    #[test]
    fn money_same_currency() {
        let a = Money::new(
            Price::new(Decimal::new(100, 0)).unwrap(),
            Currency::new("USD").unwrap(),
        );
        let b = Money::new(
            Price::new(Decimal::new(200, 0)).unwrap(),
            Currency::new("USD").unwrap(),
        );
        assert!(a.same_currency(&b));
    }

    #[test]
    fn money_different_currency() {
        let a = Money::new(
            Price::new(Decimal::new(100, 0)).unwrap(),
            Currency::new("USD").unwrap(),
        );
        let b = Money::new(
            Price::new(Decimal::new(200, 0)).unwrap(),
            Currency::new("KRW").unwrap(),
        );
        assert!(!a.same_currency(&b));
    }

    #[test]
    fn money_display() {
        let m = Money::new(
            Price::new(Decimal::new(1999, 2)).unwrap(),
            Currency::new("USD").unwrap(),
        );
        assert_eq!(m.to_string(), "19.99 USD");
    }
}
