use crate::gateway::traits::PaymentGateway;
use crate::payments::service::{find_posted_authorization, record_capture, write_outbox};
use shared::db::PgPool;
use shared::errors::AppError;
use shared::events::{EventEnvelope, EventType};
use sqlx::PgConnection;

const MAX_CAPTURE_RETRIES: u32 = 10;

/// Handle a self-consumed `PaymentCaptureRetryRequested` event.
///
/// Attempts to capture the authorized payment. On retryable failure,
/// writes another retry event (incrementing retry_count). On non-retryable
/// failure or max retries exhausted, writes `PaymentFailed`.
pub async fn handle_capture_retry(
    tx: &mut PgConnection,
    pool: &PgPool,
    gateway: &dyn PaymentGateway,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = envelope.payload_uuid("order_id")?;
    let retry_count = envelope.payload["retry_count"].as_u64().unwrap_or(0) as u32;

    if retry_count >= MAX_CAPTURE_RETRIES {
        tracing::error!(
            order_id = %order_id,
            retry_count,
            "Capture retries exhausted, writing PaymentFailed"
        );
        return write_outbox(tx, order_id, EventType::PaymentFailed, || {
            serde_json::json!({
                "order_id": order_id.to_string(),
                "reason": format!("Capture failed after {retry_count} retries"),
            })
        })
        .await;
    }

    // Read phase (pool).
    let (_, gateway_ref, auth_currency, amount) =
        find_posted_authorization(pool, order_id, "capture").await?;

    // Gateway call (no DB held).
    match gateway.capture(&gateway_ref).await {
        Ok(_) => {
            let idempotency_key = format!("capture:{order_id}");
            record_capture(
                tx,
                order_id,
                &auth_currency,
                amount,
                &idempotency_key,
                &gateway_ref,
            )
            .await
        }
        Err(gw_err) if gw_err.is_retryable => {
            tracing::warn!(
                order_id = %order_id,
                retry_count = retry_count + 1,
                error = %gw_err,
                "Capture retry failed, re-queuing"
            );
            write_outbox(
                tx,
                order_id,
                EventType::PaymentCaptureRetryRequested,
                || {
                    serde_json::json!({
                        "order_id": order_id.to_string(),
                        "retry_count": retry_count + 1,
                        "reason": gw_err.message,
                    })
                },
            )
            .await
        }
        Err(gw_err) => {
            tracing::error!(
                order_id = %order_id,
                error = %gw_err,
                "Non-retryable capture failure during retry"
            );
            write_outbox(tx, order_id, EventType::PaymentFailed, || {
                serde_json::json!({
                    "order_id": order_id.to_string(),
                    "reason": format!("Capture permanently failed: {}", gw_err.message),
                })
            })
            .await
        }
    }
}
