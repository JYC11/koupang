use shared::errors::AppError;
use std::fmt;

// Re-export shared money types. PriceSnapshot is a domain alias for Price.
pub use shared::new_types::money::Currency;
pub use shared::new_types::money::Price as PriceSnapshot;

// ── Quantity (1–99) ────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quantity(u32);

impl Quantity {
    pub fn new(value: u32) -> Result<Self, AppError> {
        if value == 0 {
            return Err(AppError::BadRequest(
                "Quantity must be at least 1".to_string(),
            ));
        }
        if value > 99 {
            return Err(AppError::BadRequest(
                "Quantity must not exceed 99".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// PriceSnapshot and Currency are re-exported from shared::new_types::money above.

// ── CartProductName ───────────────────────────────────────

shared::validated_name!(CartProductName, "Product name", 500);

// ── Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Quantity ──────────────────────────────────────────

    #[test]
    fn quantity_valid() {
        assert!(Quantity::new(1).is_ok());
        assert!(Quantity::new(50).is_ok());
        assert!(Quantity::new(99).is_ok());
    }

    #[test]
    fn quantity_rejects_zero() {
        assert!(Quantity::new(0).is_err());
    }

    #[test]
    fn quantity_rejects_over_99() {
        assert!(Quantity::new(100).is_err());
    }

    // PriceSnapshot and Currency tests live in shared::new_types::money.

    // ── CartProductName ──────────────────────────────────

    #[test]
    fn cart_product_name_valid() {
        assert!(CartProductName::new("Widget Pro").is_ok());
    }

    #[test]
    fn cart_product_name_rejects_empty() {
        assert!(CartProductName::new("").is_err());
        assert!(CartProductName::new("   ").is_err());
    }

    #[test]
    fn cart_product_name_rejects_too_long() {
        let long = "a".repeat(501);
        assert!(CartProductName::new(&long).is_err());
    }
}
