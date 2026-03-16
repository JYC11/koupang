use crate::consumers::{inventory_events, order_events};
use crate::gateway::mock::MockPaymentGateway;
use crate::gateway::traits::PaymentGateway;
use shared::db::PgPool;
use shared::events::consumer::{EventHandler, HandlerError};
use shared::events::{EventEnvelope, EventType};
use std::sync::Arc;

/// EventHandler impl for payment's Kafka consumer (catalog.events + orders.events topics).
///
/// Holds PgPool for read-phase queries (released before gateway calls) so the
/// consumer's tx isn't held open during external HTTP calls to the gateway.
pub struct PaymentEventHandler {
    pool: PgPool,
    gateway: Arc<dyn PaymentGateway>,
}

impl PaymentEventHandler {
    pub fn new(pool: PgPool, gateway: Arc<dyn PaymentGateway>) -> Self {
        Self { pool, gateway }
    }

    pub fn with_mock_gateway(pool: PgPool) -> Self {
        Self {
            pool,
            gateway: Arc::new(MockPaymentGateway::always_succeeds()),
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for PaymentEventHandler {
    async fn handle(
        &self,
        envelope: &EventEnvelope,
        tx: &mut sqlx::PgConnection,
    ) -> Result<(), HandlerError> {
        match envelope.metadata.event_type {
            EventType::InventoryReserved => inventory_events::handle_inventory_reserved(
                tx,
                &self.pool,
                self.gateway.as_ref(),
                envelope,
            )
            .await
            .map_err(|e| HandlerError::transient(e.to_string())),
            EventType::OrderConfirmed => order_events::handle_order_confirmed(
                tx,
                &self.pool,
                self.gateway.as_ref(),
                envelope,
            )
            .await
            .map_err(|e| HandlerError::transient(e.to_string())),
            EventType::OrderCancelled => order_events::handle_order_cancelled(
                tx,
                &self.pool,
                self.gateway.as_ref(),
                envelope,
            )
            .await
            .map_err(|e| HandlerError::transient(e.to_string())),
            _ => Ok(()),
        }
    }
}
