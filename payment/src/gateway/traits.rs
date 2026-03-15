use async_trait::async_trait;
use rust_decimal::Decimal;
use shared::errors::AppError;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GatewayAuthResult {
    pub gateway_reference: String,
    pub approved_amount: Decimal,
    pub status: GatewayStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayStatus {
    Success,
    Failed,
}

#[derive(Debug, Clone)]
pub struct GatewayCaptureResult {
    pub status: GatewayStatus,
}

#[derive(Debug, Clone)]
pub struct GatewayVoidResult {
    pub status: GatewayStatus,
}

#[derive(Debug, Clone)]
pub struct GatewayRefundResult {
    pub status: GatewayStatus,
}

#[async_trait]
pub trait PaymentGateway: Send + Sync {
    async fn authorize(
        &self,
        idempotency_key: &str,
        order_id: Uuid,
        amount: Decimal,
        currency: &str,
    ) -> Result<GatewayAuthResult, AppError>;

    async fn capture(&self, gateway_reference: &str) -> Result<GatewayCaptureResult, AppError>;

    async fn void(&self, gateway_reference: &str) -> Result<GatewayVoidResult, AppError>;

    async fn refund(
        &self,
        gateway_reference: &str,
        amount: Decimal,
    ) -> Result<GatewayRefundResult, AppError>;
}
