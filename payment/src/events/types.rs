use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Payload for PaymentAuthorized events on payments.events topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentAuthorizedPayload {
    pub order_id: Uuid,
    pub payment_id: Uuid,
    pub gateway_reference: String,
}

/// Payload for PaymentFailed events on payments.events topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentFailedPayload {
    pub order_id: Uuid,
    pub reason: String,
}

/// Payload for PaymentCaptured events on payments.events topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentCapturedPayload {
    pub order_id: Uuid,
}

/// Payload for PaymentVoided events on payments.events topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentVoidedPayload {
    pub order_id: Uuid,
}
