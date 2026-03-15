use async_trait::async_trait;
use rust_decimal::Decimal;
use shared::errors::AppError;
use uuid::Uuid;

use super::traits::{
    GatewayAuthResult, GatewayCaptureResult, GatewayRefundResult, GatewayStatus, GatewayVoidResult,
    PaymentGateway,
};

pub struct MockPaymentGateway {
    always_succeed: bool,
    tampered_amount: Option<Decimal>,
}

impl MockPaymentGateway {
    pub fn always_succeeds() -> Self {
        Self {
            always_succeed: true,
            tampered_amount: None,
        }
    }

    pub fn always_fails() -> Self {
        Self {
            always_succeed: false,
            tampered_amount: None,
        }
    }

    /// Returns a different amount than requested (for tamper detection tests).
    pub fn tampered_amount(amount: Decimal) -> Self {
        Self {
            always_succeed: true,
            tampered_amount: Some(amount),
        }
    }
}

#[async_trait]
impl PaymentGateway for MockPaymentGateway {
    async fn authorize(
        &self,
        _idempotency_key: &str,
        _order_id: Uuid,
        amount: Decimal,
        _currency: &str,
    ) -> Result<GatewayAuthResult, AppError> {
        // Simulate 10ms latency
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        if !self.always_succeed {
            return Err(AppError::InternalServerError(
                "Payment gateway declined".to_string(),
            ));
        }

        let approved_amount = self.tampered_amount.unwrap_or(amount);

        Ok(GatewayAuthResult {
            gateway_reference: format!("mock-auth-{}", Uuid::now_v7()),
            approved_amount,
            status: GatewayStatus::Success,
        })
    }

    async fn capture(&self, _gateway_reference: &str) -> Result<GatewayCaptureResult, AppError> {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        if !self.always_succeed {
            return Err(AppError::InternalServerError("Capture failed".to_string()));
        }

        Ok(GatewayCaptureResult {
            status: GatewayStatus::Success,
        })
    }

    async fn void(&self, _gateway_reference: &str) -> Result<GatewayVoidResult, AppError> {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        if !self.always_succeed {
            return Err(AppError::InternalServerError("Void failed".to_string()));
        }

        Ok(GatewayVoidResult {
            status: GatewayStatus::Success,
        })
    }

    async fn refund(
        &self,
        _gateway_reference: &str,
        _amount: Decimal,
    ) -> Result<GatewayRefundResult, AppError> {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        if !self.always_succeed {
            return Err(AppError::InternalServerError("Refund failed".to_string()));
        }

        Ok(GatewayRefundResult {
            status: GatewayStatus::Success,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_gateway_always_succeeds() {
        let gw = MockPaymentGateway::always_succeeds();
        let result = gw
            .authorize("key-1", Uuid::now_v7(), Decimal::new(4998, 2), "USD")
            .await;
        assert!(result.is_ok());
        let auth = result.unwrap();
        assert_eq!(auth.approved_amount, Decimal::new(4998, 2));
        assert_eq!(auth.status, GatewayStatus::Success);
        assert!(auth.gateway_reference.starts_with("mock-auth-"));
    }

    #[tokio::test]
    async fn mock_gateway_always_fails() {
        let gw = MockPaymentGateway::always_fails();
        let result = gw
            .authorize("key-1", Uuid::now_v7(), Decimal::new(4998, 2), "USD")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mock_gateway_tampered_amount() {
        let gw = MockPaymentGateway::tampered_amount(Decimal::new(9999, 2));
        let result = gw
            .authorize("key-1", Uuid::now_v7(), Decimal::new(4998, 2), "USD")
            .await
            .unwrap();
        assert_eq!(result.approved_amount, Decimal::new(9999, 2));
    }

    #[tokio::test]
    async fn mock_gateway_capture() {
        let gw = MockPaymentGateway::always_succeeds();
        let result = gw.capture("mock-auth-123").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn mock_gateway_void() {
        let gw = MockPaymentGateway::always_succeeds();
        let result = gw.void("mock-auth-123").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn mock_gateway_refund() {
        let gw = MockPaymentGateway::always_succeeds();
        let result = gw.refund("mock-auth-123", Decimal::new(4998, 2)).await;
        assert!(result.is_ok());
    }
}
