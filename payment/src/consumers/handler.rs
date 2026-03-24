use std::sync::Arc;
use std::time::Duration;

use crate::consumers::{capture_retry, inventory_events, order_events};
use crate::gateway::mock::MockPaymentGateway;
use crate::gateway::traits::PaymentGateway;
use shared::db::PgPool;
use shared::distributed_lock::{DistributedLock, LockError};
use shared::events::consumer::{EventHandler, HandlerError};
use shared::events::{EventEnvelope, EventType};

/// EventHandler impl for payment's Kafka consumer.
///
/// Holds PgPool for read-phase queries (released before gateway calls) so the
/// consumer's tx isn't held open during external HTTP calls to the gateway.
/// Optional distributed lock prevents concurrent processing of the same order.
pub struct PaymentEventHandler {
    pool: PgPool,
    gateway: Arc<dyn PaymentGateway>,
    lock: Option<DistributedLock>,
}

impl PaymentEventHandler {
    pub fn new(
        pool: PgPool,
        gateway: Arc<dyn PaymentGateway>,
        redis: Option<redis::aio::ConnectionManager>,
    ) -> Self {
        Self {
            pool,
            gateway,
            lock: redis.map(DistributedLock::new),
        }
    }

    pub fn with_mock_gateway(pool: PgPool) -> Self {
        Self {
            pool,
            gateway: Arc::new(MockPaymentGateway::always_succeeds()),
            lock: None,
        }
    }
}

/// TTL for the distributed lock based on operation type.
fn lock_ttl(event_type: &EventType) -> Duration {
    match event_type {
        // Capture and retry may involve slow gateway calls.
        EventType::OrderConfirmed | EventType::PaymentCaptureRetryRequested => {
            Duration::from_secs(60)
        }
        _ => Duration::from_secs(30),
    }
}

#[async_trait::async_trait]
impl EventHandler for PaymentEventHandler {
    async fn handle(
        &self,
        envelope: &EventEnvelope,
        tx: &mut sqlx::PgConnection,
    ) -> Result<(), HandlerError> {
        // Only payment-relevant events need lock + dispatch.
        let order_id = match &envelope.metadata.event_type {
            EventType::InventoryReserved
            | EventType::OrderConfirmed
            | EventType::OrderCancelled
            | EventType::PaymentCaptureRetryRequested => envelope
                .payload_uuid("order_id")
                .map_err(|e| HandlerError::permanent(e.to_string()))?,
            _ => return Ok(()),
        };

        // Acquire distributed lock (fail-open if Redis unavailable).
        let lock_key = format!("payment:{order_id}");
        let ttl = lock_ttl(&envelope.metadata.event_type);
        let guard = match &self.lock {
            Some(lock) => match lock.acquire(&lock_key, ttl).await {
                Ok(guard) => Some(guard),
                Err(LockError::AlreadyHeld) => {
                    return Err(HandlerError::transient(format!(
                        "payment:{order_id} in-flight, will retry"
                    )));
                }
                Err(LockError::RedisUnavailable(e)) => {
                    tracing::warn!(
                        order_id = %order_id,
                        error = %e,
                        "Redis lock unavailable, proceeding without lock"
                    );
                    None
                }
            },
            None => None,
        };

        // Dispatch to handler.
        let result = match &envelope.metadata.event_type {
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
            EventType::PaymentCaptureRetryRequested => {
                capture_retry::handle_capture_retry(tx, &self.pool, self.gateway.as_ref(), envelope)
                    .await
                    .map_err(|e| HandlerError::transient(e.to_string()))
            }
            _ => Ok(()),
        };

        // Best-effort lock release (TTL is the safety net).
        if let Some(guard) = guard {
            if let Err(e) = guard.release().await {
                tracing::warn!(order_id = %order_id, error = %e, "Failed to release lock");
            }
        }

        result
    }
}
