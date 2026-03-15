use shared::errors::AppError;
use std::fmt;

#[derive(Debug)]
pub enum CartError {
    ValidationFailed(String),
    CartFull { max: u64 },
    ItemNotFound(String),
    CheckoutNotReady(String),
    Infra(AppError),
}

impl fmt::Display for CartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValidationFailed(msg) => write!(f, "Validation failed: {msg}"),
            Self::CartFull { max } => write!(f, "Cart is full (max {max} items)"),
            Self::ItemNotFound(msg) => write!(f, "Item not found: {msg}"),
            Self::CheckoutNotReady(msg) => write!(f, "Checkout not ready: {msg}"),
            Self::Infra(e) => write!(f, "{e}"),
        }
    }
}

impl From<AppError> for CartError {
    fn from(e: AppError) -> Self {
        Self::Infra(e)
    }
}

impl From<CartError> for AppError {
    fn from(e: CartError) -> Self {
        match e {
            CartError::ValidationFailed(msg) => AppError::BadRequest(msg),
            CartError::CartFull { max } => {
                AppError::BadRequest(format!("Cart is full (max {max} items)"))
            }
            CartError::ItemNotFound(msg) => AppError::NotFound(msg),
            CartError::CheckoutNotReady(msg) => AppError::BadRequest(msg),
            CartError::Infra(e) => e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cart_full_maps_to_bad_request() {
        let err: AppError = CartError::CartFull { max: 50 }.into();
        assert!(matches!(err, AppError::BadRequest(_)));
        assert!(err.to_string().contains("50"));
    }

    #[test]
    fn item_not_found_maps_to_not_found() {
        let err: AppError = CartError::ItemNotFound("sku-123".to_string()).into();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn checkout_not_ready_maps_to_bad_request() {
        let err: AppError = CartError::CheckoutNotReady("empty cart".to_string()).into();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn infra_passthrough() {
        let err: AppError =
            CartError::Infra(AppError::InternalServerError("db down".to_string())).into();
        assert!(matches!(err, AppError::InternalServerError(_)));
    }
}
