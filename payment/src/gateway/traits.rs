use async_trait::async_trait;
use rust_decimal::Decimal;
use std::fmt;
use uuid::Uuid;

// ── Gateway result types ────────────────────────────────────

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

// ── Gateway error (structured, from Hyperswitch pattern) ────

/// Structured error from the payment gateway. `is_retryable` drives retry
/// logic and circuit breaker classification (only infra failures trip the
/// breaker, business declines do not).
#[derive(Debug, Clone)]
pub struct GatewayError {
    /// Machine-readable error code (e.g. "DECLINED", "TIMEOUT", "INSUFFICIENT_FUNDS").
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Gateway-specific detail (e.g. processor decline reason).
    pub reason: Option<String>,
    /// Whether this error is transient and the operation should be retried.
    pub is_retryable: bool,
}

impl GatewayError {
    pub fn declined(message: impl Into<String>) -> Self {
        Self {
            code: "DECLINED".to_string(),
            message: message.into(),
            reason: None,
            is_retryable: false,
        }
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            code: "TIMEOUT".to_string(),
            message: message.into(),
            reason: None,
            is_retryable: true,
        }
    }
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if let Some(reason) = &self.reason {
            write!(f, " (reason: {reason})")?;
        }
        Ok(())
    }
}

impl std::error::Error for GatewayError {}

// ── Gateway trait ───────────────────────────────────────────

#[async_trait]
pub trait PaymentGateway: Send + Sync {
    async fn authorize(
        &self,
        idempotency_key: &str,
        order_id: Uuid,
        amount: Decimal,
        currency: &str,
    ) -> Result<GatewayAuthResult, GatewayError>;

    async fn capture(&self, gateway_reference: &str) -> Result<GatewayCaptureResult, GatewayError>;

    async fn void(&self, gateway_reference: &str) -> Result<GatewayVoidResult, GatewayError>;

    async fn refund(
        &self,
        gateway_reference: &str,
        amount: Decimal,
    ) -> Result<GatewayRefundResult, GatewayError>;
}
