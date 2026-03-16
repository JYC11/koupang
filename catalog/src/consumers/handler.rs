use crate::consumers::order_events;
use shared::db::PgPool;
use shared::events::consumer::{EventHandler, HandlerError};
use shared::events::{EventEnvelope, EventType};

/// EventHandler impl for catalog's Kafka consumer (orders.events topic).
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
            EventType::OrderCreated => order_events::handle_order_created(tx, &self.pool, envelope)
                .await
                .map_err(|e| HandlerError::transient(e.to_string())),
            EventType::OrderCancelled => order_events::handle_order_cancelled(tx, envelope)
                .await
                .map_err(|e| HandlerError::transient(e.to_string())),
            _ => Ok(()),
        }
    }
}
