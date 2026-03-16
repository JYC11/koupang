use crate::consumers::{inventory_events, payment_events};
use shared::events::consumer::{EventHandler, HandlerError};
use shared::events::{EventEnvelope, EventType};

/// EventHandler impl for order's Kafka consumer (catalog.events + payments.events topics).
pub struct OrderEventHandler;

impl OrderEventHandler {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl EventHandler for OrderEventHandler {
    async fn handle(
        &self,
        envelope: &EventEnvelope,
        tx: &mut sqlx::PgConnection,
    ) -> Result<(), HandlerError> {
        match envelope.metadata.event_type {
            EventType::InventoryReserved => {
                inventory_events::handle_inventory_reserved(tx, envelope)
                    .await
                    .map_err(|e| HandlerError::transient(e.to_string()))
            }
            EventType::InventoryReservationFailed => {
                inventory_events::handle_inventory_reservation_failed(tx, envelope)
                    .await
                    .map_err(|e| HandlerError::transient(e.to_string()))
            }
            EventType::PaymentAuthorized => payment_events::handle_payment_authorized(tx, envelope)
                .await
                .map_err(|e| HandlerError::transient(e.to_string())),
            EventType::PaymentFailed => payment_events::handle_payment_failed(tx, envelope)
                .await
                .map_err(|e| HandlerError::transient(e.to_string())),
            EventType::PaymentTimedOut => payment_events::handle_payment_timed_out(tx, envelope)
                .await
                .map_err(|e| HandlerError::transient(e.to_string())),
            _ => Ok(()),
        }
    }
}
