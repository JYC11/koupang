use crate::orders::error::OrderError;
use serde::{Deserialize, Serialize};
use shared::errors::AppError;
use std::fmt;

// Re-export shared money types so order code imports from one place.
pub use shared::new_types::money::{Currency, Price};

shared::valid_id!(OrderId);
shared::valid_id!(OrderItemId);

// ── OrderStatus ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    InventoryReserved,
    PaymentAuthorized,
    Confirmed,
    Shipped,
    Delivered,
    Cancelled,
    Returned,
}

impl OrderStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InventoryReserved => "inventory_reserved",
            Self::PaymentAuthorized => "payment_authorized",
            Self::Confirmed => "confirmed",
            Self::Shipped => "shipped",
            Self::Delivered => "delivered",
            Self::Cancelled => "cancelled",
            Self::Returned => "returned",
        }
    }

    /// Validates a state transition. Returns the target status on success.
    pub fn transition_to(&self, target: &OrderStatus) -> Result<OrderStatus, OrderError> {
        let allowed = match self {
            Self::Pending => matches!(target, Self::InventoryReserved | Self::Cancelled),
            Self::InventoryReserved => matches!(target, Self::PaymentAuthorized | Self::Cancelled),
            Self::PaymentAuthorized => matches!(target, Self::Confirmed | Self::Cancelled),
            Self::Confirmed => matches!(target, Self::Shipped | Self::Cancelled),
            Self::Shipped => matches!(target, Self::Delivered | Self::Cancelled),
            Self::Delivered => matches!(target, Self::Returned),
            Self::Cancelled | Self::Returned => false,
        };

        if allowed {
            Ok(target.clone())
        } else {
            Err(OrderError::InvalidTransition {
                from: self.clone(),
                to: target.clone(),
            })
        }
    }

    pub fn can_cancel(&self) -> bool {
        matches!(
            self,
            Self::Pending
                | Self::InventoryReserved
                | Self::PaymentAuthorized
                | Self::Confirmed
                | Self::Shipped
        )
    }
}

impl fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── ShippingAddress ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShippingAddress {
    pub street: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

const MAX_STREET_LEN: usize = 500;
const MAX_CITY_LEN: usize = 200;
const MAX_POSTAL_CODE_LEN: usize = 20;
const MAX_COUNTRY_LEN: usize = 3;

impl ShippingAddress {
    pub fn new(req: ShippingAddressReq) -> Result<Self, AppError> {
        let street = req.street.trim().to_string();
        let city = req.city.trim().to_string();
        let state = req.state.trim().to_string();
        let postal_code = req.postal_code.trim().to_string();
        let country = req.country.trim().to_string();

        if street.is_empty() || city.is_empty() || postal_code.is_empty() || country.is_empty() {
            return Err(AppError::BadRequest(
                "Shipping address fields (street, city, postal_code, country) must not be empty"
                    .to_string(),
            ));
        }

        if street.len() > MAX_STREET_LEN {
            return Err(AppError::BadRequest(format!(
                "Street must not exceed {MAX_STREET_LEN} characters"
            )));
        }
        if city.len() > MAX_CITY_LEN {
            return Err(AppError::BadRequest(format!(
                "City must not exceed {MAX_CITY_LEN} characters"
            )));
        }
        if postal_code.len() > MAX_POSTAL_CODE_LEN {
            return Err(AppError::BadRequest(format!(
                "Postal code must not exceed {MAX_POSTAL_CODE_LEN} characters"
            )));
        }
        if country.len() > MAX_COUNTRY_LEN {
            return Err(AppError::BadRequest(format!(
                "Country must not exceed {MAX_COUNTRY_LEN} characters"
            )));
        }

        Ok(Self {
            street,
            city,
            state,
            postal_code,
            country,
        })
    }

    /// Check if the address has all required fields populated.
    pub fn is_complete(&self) -> bool {
        !self.street.is_empty()
            && !self.city.is_empty()
            && !self.postal_code.is_empty()
            && !self.country.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShippingAddressReq {
    pub street: String,
    pub city: String,
    #[serde(default)]
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

// ── IdempotencyKey ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(AppError::BadRequest(
                "Idempotency key must not be empty".to_string(),
            ));
        }
        if trimmed.len() > 255 {
            return Err(AppError::BadRequest(
                "Idempotency key must not exceed 255 characters".to_string(),
            ));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Quantity ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct Quantity(i32);

pub const MAX_ORDER_QUANTITY: i32 = 9999;

impl Quantity {
    pub fn new(value: i32) -> Result<Self, AppError> {
        if value <= 0 {
            return Err(AppError::BadRequest(
                "Quantity must be greater than 0".to_string(),
            ));
        }
        if value > MAX_ORDER_QUANTITY {
            return Err(AppError::BadRequest(format!(
                "Quantity must not exceed {MAX_ORDER_QUANTITY}"
            )));
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> i32 {
        self.0
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Price and Currency are re-exported from shared::new_types::money above.

// ── Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── OrderStatus transitions ───────────────────────────

    #[test]
    fn pending_can_transition_to_inventory_reserved() {
        let result = OrderStatus::Pending.transition_to(&OrderStatus::InventoryReserved);
        assert!(result.is_ok());
    }

    #[test]
    fn pending_can_transition_to_cancelled() {
        let result = OrderStatus::Pending.transition_to(&OrderStatus::Cancelled);
        assert!(result.is_ok());
    }

    #[test]
    fn pending_cannot_transition_to_confirmed() {
        let result = OrderStatus::Pending.transition_to(&OrderStatus::Confirmed);
        assert!(result.is_err());
    }

    #[test]
    fn inventory_reserved_to_payment_authorized() {
        let result = OrderStatus::InventoryReserved.transition_to(&OrderStatus::PaymentAuthorized);
        assert!(result.is_ok());
    }

    #[test]
    fn payment_authorized_to_confirmed() {
        let result = OrderStatus::PaymentAuthorized.transition_to(&OrderStatus::Confirmed);
        assert!(result.is_ok());
    }

    #[test]
    fn confirmed_to_shipped() {
        let result = OrderStatus::Confirmed.transition_to(&OrderStatus::Shipped);
        assert!(result.is_ok());
    }

    #[test]
    fn shipped_to_delivered() {
        let result = OrderStatus::Shipped.transition_to(&OrderStatus::Delivered);
        assert!(result.is_ok());
    }

    #[test]
    fn delivered_to_returned() {
        let result = OrderStatus::Delivered.transition_to(&OrderStatus::Returned);
        assert!(result.is_ok());
    }

    #[test]
    fn cancelled_cannot_transition() {
        assert!(
            OrderStatus::Cancelled
                .transition_to(&OrderStatus::Pending)
                .is_err()
        );
        assert!(
            OrderStatus::Cancelled
                .transition_to(&OrderStatus::Confirmed)
                .is_err()
        );
    }

    #[test]
    fn returned_cannot_transition() {
        assert!(
            OrderStatus::Returned
                .transition_to(&OrderStatus::Pending)
                .is_err()
        );
    }

    #[test]
    fn can_cancel_from_valid_states() {
        assert!(OrderStatus::Pending.can_cancel());
        assert!(OrderStatus::InventoryReserved.can_cancel());
        assert!(OrderStatus::PaymentAuthorized.can_cancel());
        assert!(OrderStatus::Confirmed.can_cancel());
        assert!(OrderStatus::Shipped.can_cancel());
    }

    #[test]
    fn cannot_cancel_terminal_states() {
        assert!(!OrderStatus::Cancelled.can_cancel());
        assert!(!OrderStatus::Returned.can_cancel());
        assert!(!OrderStatus::Delivered.can_cancel());
    }

    // ── IdempotencyKey ────────────────────────────────────

    #[test]
    fn idempotency_key_valid() {
        assert!(IdempotencyKey::new("order-123-abc").is_ok());
    }

    #[test]
    fn idempotency_key_rejects_empty() {
        assert!(IdempotencyKey::new("").is_err());
        assert!(IdempotencyKey::new("   ").is_err());
    }

    #[test]
    fn idempotency_key_rejects_too_long() {
        let long = "a".repeat(256);
        assert!(IdempotencyKey::new(&long).is_err());
    }

    // ── Quantity ──────────────────────────────────────────

    #[test]
    fn quantity_valid() {
        assert!(Quantity::new(1).is_ok());
        assert!(Quantity::new(100).is_ok());
        assert!(Quantity::new(9999).is_ok());
    }

    #[test]
    fn quantity_rejects_zero_and_negative() {
        assert!(Quantity::new(0).is_err());
        assert!(Quantity::new(-1).is_err());
    }

    #[test]
    fn quantity_rejects_over_max() {
        assert!(Quantity::new(10000).is_err());
    }

    // Price and Currency tests live in shared::new_types::money.

    // ── ShippingAddress ───────────────────────────────────

    #[test]
    fn shipping_address_valid() {
        let req = ShippingAddressReq {
            street: "123 Main St".to_string(),
            city: "Seoul".to_string(),
            state: "".to_string(),
            postal_code: "06000".to_string(),
            country: "KR".to_string(),
        };
        assert!(ShippingAddress::new(req).is_ok());
    }

    #[test]
    fn shipping_address_rejects_empty_street() {
        let req = ShippingAddressReq {
            street: "   ".to_string(),
            city: "Seoul".to_string(),
            state: "".to_string(),
            postal_code: "06000".to_string(),
            country: "KR".to_string(),
        };
        assert!(ShippingAddress::new(req).is_err());
    }

    #[test]
    fn shipping_address_rejects_too_long_street() {
        let req = ShippingAddressReq {
            street: "a".repeat(501),
            city: "Seoul".to_string(),
            state: "".to_string(),
            postal_code: "06000".to_string(),
            country: "KR".to_string(),
        };
        assert!(ShippingAddress::new(req).is_err());
    }

    #[test]
    fn shipping_address_rejects_too_long_country() {
        let req = ShippingAddressReq {
            street: "123 Main St".to_string(),
            city: "Seoul".to_string(),
            state: "".to_string(),
            postal_code: "06000".to_string(),
            country: "KOREA".to_string(),
        };
        assert!(ShippingAddress::new(req).is_err());
    }

    // ── OrderStatus serialization ─────────────────────────

    #[test]
    fn order_status_as_str() {
        assert_eq!(OrderStatus::Pending.as_str(), "pending");
        assert_eq!(
            OrderStatus::InventoryReserved.as_str(),
            "inventory_reserved"
        );
        assert_eq!(
            OrderStatus::PaymentAuthorized.as_str(),
            "payment_authorized"
        );
        assert_eq!(OrderStatus::Confirmed.as_str(), "confirmed");
        assert_eq!(OrderStatus::Shipped.as_str(), "shipped");
        assert_eq!(OrderStatus::Delivered.as_str(), "delivered");
        assert_eq!(OrderStatus::Cancelled.as_str(), "cancelled");
        assert_eq!(OrderStatus::Returned.as_str(), "returned");
    }
}
