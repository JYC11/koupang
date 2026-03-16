use crate::consumers::order_events;
use shared::db::PgPool;
use shared::events::consumer::{EventHandler, HandlerError};
use shared::events::{EventEnvelope, EventType};

/// EventHandler impl for catalog's Kafka consumer (orders.events topic).
///
/// Holds a PgPool in addition to the consumer-provided tx because the inventory
/// reservation failure path needs to write InventoryReservationFailed on a *separate*
/// transaction — the main tx will be rolled back by the consumer on error, but the
/// failure event must survive to notify the order service (saga compensation pattern).
pub struct CatalogEventHandler {
    pool: PgPool,
}

impl CatalogEventHandler {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl EventHandler for CatalogEventHandler {
    async fn handle(
        &self,
        envelope: &EventEnvelope,
        tx: &mut sqlx::PgConnection,
    ) -> Result<(), HandlerError> {
        match envelope.metadata.event_type {
            EventType::OrderCreated => {
                order_events::handle_order_created(tx, &self.pool, envelope).await
            }
            EventType::OrderCancelled => order_events::handle_order_cancelled(tx, envelope)
                .await
                .map_err(|e| HandlerError::transient(e.to_string())),
            _ => Ok(()),
        }
    }
}
