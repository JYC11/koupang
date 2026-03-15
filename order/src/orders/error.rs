use crate::orders::value_objects::OrderStatus;
use shared::errors::AppError;
use std::fmt;

#[derive(Debug)]
pub enum OrderError {
    ValidationFailed(String),
    InvalidTransition { from: OrderStatus, to: OrderStatus },
    CancellationDenied(String),
    NotFound(String),
    AccessDenied(String),
    Infra(AppError),
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValidationFailed(msg) => write!(f, "Validation failed: {msg}"),
            Self::InvalidTransition { from, to } => {
                write!(f, "Invalid order status transition: {from} → {to}")
            }
            Self::CancellationDenied(msg) => write!(f, "Cancellation denied: {msg}"),
            Self::NotFound(msg) => write!(f, "Not found: {msg}"),
            Self::AccessDenied(msg) => write!(f, "Access denied: {msg}"),
            Self::Infra(e) => write!(f, "{e}"),
        }
    }
}

impl From<AppError> for OrderError {
    fn from(e: AppError) -> Self {
        Self::Infra(e)
    }
}

impl From<OrderError> for AppError {
    fn from(e: OrderError) -> Self {
        match e {
            OrderError::ValidationFailed(msg) => AppError::BadRequest(msg),
            OrderError::InvalidTransition { from, to } => {
                AppError::BadRequest(format!("Invalid order status transition: {from} → {to}"))
            }
            OrderError::CancellationDenied(msg) => AppError::BadRequest(msg),
            OrderError::NotFound(msg) => AppError::NotFound(msg),
            OrderError::AccessDenied(msg) => AppError::Forbidden(msg),
            OrderError::Infra(e) => e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_failed_maps_to_bad_request() {
        let err: AppError = OrderError::ValidationFailed("bad input".to_string()).into();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn invalid_transition_maps_to_bad_request() {
        let err: AppError = OrderError::InvalidTransition {
            from: OrderStatus::Cancelled,
            to: OrderStatus::Confirmed,
        }
        .into();
        assert!(matches!(err, AppError::BadRequest(_)));
        assert!(err.to_string().contains("cancelled"));
    }

    #[test]
    fn not_found_maps_to_not_found() {
        let err: AppError = OrderError::NotFound("order xyz".to_string()).into();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn access_denied_maps_to_forbidden() {
        let err: AppError = OrderError::AccessDenied("not your order".to_string()).into();
        assert!(matches!(err, AppError::Forbidden(_)));
    }

    #[test]
    fn infra_passthrough() {
        let err: AppError =
            OrderError::Infra(AppError::InternalServerError("db down".to_string())).into();
        assert!(matches!(err, AppError::InternalServerError(_)));
    }
}
