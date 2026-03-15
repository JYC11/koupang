use crate::ledger::value_objects::PaymentState;
use rust_decimal::Decimal;
use shared::errors::AppError;
use std::fmt;

#[derive(Debug)]
pub enum PaymentError {
    ValidationFailed(String),
    InvalidState {
        operation: String,
        state: PaymentState,
    },
    AmountTampered {
        requested: Decimal,
        approved: Decimal,
    },
    NotFound(String),
    GatewayError(String),
    Infra(AppError),
}

impl fmt::Display for PaymentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValidationFailed(msg) => write!(f, "Validation failed: {msg}"),
            Self::InvalidState { operation, state } => {
                write!(f, "Cannot {operation} payment in state: {state}")
            }
            Self::AmountTampered {
                requested,
                approved,
            } => {
                write!(
                    f,
                    "Amount tampering detected: requested={requested}, approved={approved}"
                )
            }
            Self::NotFound(msg) => write!(f, "Not found: {msg}"),
            Self::GatewayError(msg) => write!(f, "Gateway error: {msg}"),
            Self::Infra(e) => write!(f, "{e}"),
        }
    }
}

impl From<AppError> for PaymentError {
    fn from(e: AppError) -> Self {
        Self::Infra(e)
    }
}

impl From<PaymentError> for AppError {
    fn from(e: PaymentError) -> Self {
        match e {
            PaymentError::ValidationFailed(msg) => AppError::BadRequest(msg),
            PaymentError::InvalidState { operation, state } => {
                AppError::BadRequest(format!("Cannot {operation} payment in state: {state}"))
            }
            PaymentError::AmountTampered {
                requested,
                approved,
            } => AppError::BadRequest(format!(
                "Amount tampering detected: requested={requested}, approved={approved}"
            )),
            PaymentError::NotFound(msg) => AppError::NotFound(msg),
            PaymentError::GatewayError(msg) => {
                AppError::InternalServerError(format!("Gateway error: {msg}"))
            }
            PaymentError::Infra(e) => e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_state_maps_to_bad_request() {
        let err: AppError = PaymentError::InvalidState {
            operation: "capture".to_string(),
            state: PaymentState::New,
        }
        .into();
        assert!(matches!(err, AppError::BadRequest(_)));
        assert!(err.to_string().contains("capture"));
    }

    #[test]
    fn amount_tampered_maps_to_bad_request() {
        let err: AppError = PaymentError::AmountTampered {
            requested: Decimal::new(100, 0),
            approved: Decimal::new(50, 0),
        }
        .into();
        assert!(matches!(err, AppError::BadRequest(_)));
        assert!(err.to_string().contains("tampering"));
    }

    #[test]
    fn not_found_maps_to_not_found() {
        let err: AppError = PaymentError::NotFound("tx-123".to_string()).into();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn gateway_error_maps_to_internal() {
        let err: AppError = PaymentError::GatewayError("timeout".to_string()).into();
        assert!(matches!(err, AppError::InternalServerError(_)));
    }

    #[test]
    fn infra_passthrough() {
        let err: AppError =
            PaymentError::Infra(AppError::InternalServerError("db down".to_string())).into();
        assert!(matches!(err, AppError::InternalServerError(_)));
    }
}
