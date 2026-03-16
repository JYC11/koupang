use crate::AppState;
use crate::gateway::traits::PaymentGateway;
use crate::ledger::repository as ledger_repo;
use crate::ledger::value_objects::{
    AccountType, EntryDirection, TransactionStatus, TransactionType,
};
use crate::payments::error::PaymentError;
use crate::payments::rules::{AuthorizationContext, authorization_rules, eval_authorization};
use rust_decimal::Decimal;
use shared::db::PgPool;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::errors::AppError;
use shared::events::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};
use shared::outbox::{OutboxInsert, insert_outbox_event};
use sqlx::PgConnection;
use uuid::Uuid;

// ── Pool-based public API (used by HTTP routes) ─────────────

/// Authorize payment for an order. Called when InventoryReserved is received.
pub async fn authorize_payment(
    state: &AppState,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
    amount: Decimal,
    currency: &str,
) -> Result<(), AppError> {
    let idempotency_key = format!("auth:{order_id}");

    let existing_tx =
        ledger_repo::get_transaction_by_idempotency_key(&state.pool, &idempotency_key).await?;
    if let Some(ref existing) = existing_tx {
        if existing.status != TransactionStatus::Discarded {
            return Ok(());
        }
    }

    let transactions = ledger_repo::list_transactions_by_order(&state.pool, order_id).await?;
    let payment_state = ledger_repo::derive_payment_state(&transactions);
    validate_authorization(amount, currency, &payment_state)?;

    let auth_result = gateway
        .authorize(&idempotency_key, order_id, amount, currency)
        .await;

    match auth_result {
        Ok(result) => {
            if result.approved_amount != amount {
                return handle_tampered_amount(state, gateway, order_id, amount, &result).await;
            }
            let currency_owned = currency.to_string();
            with_transaction(&state.pool, |tx| {
                Box::pin(async move {
                    record_authorization(
                        tx.as_executor(),
                        order_id,
                        amount,
                        &currency_owned,
                        &result.gateway_reference,
                    )
                    .await?;
                    Ok(())
                })
            })
            .await
            .map_err(|e| AppError::InternalServerError(format!("Failed to authorize: {e}")))?;
        }
        Err(e) => {
            tracing::warn!(order_id = %order_id, error = %e, "Gateway declined authorization");
            write_payment_event(state, order_id, EventType::PaymentFailed, || {
                serde_json::json!({ "order_id": order_id.to_string(), "reason": "Payment gateway declined" })
            })
            .await?;
        }
    }

    Ok(())
}

/// Capture an authorized payment.
pub async fn capture_payment(
    state: &AppState,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
) -> Result<(), AppError> {
    let (_, gateway_ref, auth_currency, amount) =
        find_posted_authorization(&state.pool, order_id, "capture").await?;

    gateway.capture(&gateway_ref).await?;

    let idempotency_key = format!("capture:{order_id}");
    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            record_capture(
                tx.as_executor(),
                order_id,
                &auth_currency,
                amount,
                &idempotency_key,
                &gateway_ref,
            )
            .await?;
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to capture payment: {e}")))
}

/// Void an authorized payment.
pub async fn void_payment(
    state: &AppState,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
) -> Result<(), AppError> {
    let (auth_tx_id, gateway_ref, auth_currency, amount) =
        find_posted_authorization(&state.pool, order_id, "void").await?;

    gateway.void(&gateway_ref).await?;

    let idempotency_key = format!("void:{order_id}");
    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            record_void(
                tx.as_executor(),
                order_id,
                auth_tx_id,
                &auth_currency,
                amount,
                &idempotency_key,
            )
            .await?;
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to void payment: {e}")))
}

// ── On-tx public API (read on pool, gateway call, write on tx) ──

/// Authorize payment: reads on pool (no held connection during gateway call), writes on tx.
pub async fn authorize_payment_on_tx(
    pool: &PgPool,
    tx: &mut PgConnection,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
    amount: Decimal,
    currency: &str,
) -> Result<(), AppError> {
    let idempotency_key = format!("auth:{order_id}");

    // Read phase (on pool — connection returned before gateway call)
    let existing = ledger_repo::get_transaction_by_idempotency_key(pool, &idempotency_key).await?;
    if let Some(ref existing) = existing {
        if existing.status != TransactionStatus::Discarded {
            return Ok(());
        }
    }

    let transactions = ledger_repo::list_transactions_by_order(pool, order_id).await?;
    let payment_state = ledger_repo::derive_payment_state(&transactions);
    validate_authorization(amount, currency, &payment_state)?;

    // Gateway call (no DB connection held)
    let auth_result = gateway
        .authorize(&idempotency_key, order_id, amount, currency)
        .await;

    // Write phase (on consumer's tx)
    match auth_result {
        Ok(result) => {
            if result.approved_amount != amount {
                if let Err(e) = gateway.void(&result.gateway_reference).await {
                    tracing::error!(order_id = %order_id, error = %e, "Failed to void tampered auth");
                }
                write_outbox(tx, order_id, EventType::PaymentFailed, || {
                    serde_json::json!({
                        "order_id": order_id.to_string(),
                        "reason": format!("Amount tampering: requested={amount}, approved={}", result.approved_amount),
                    })
                })
                .await?;
                return Err(PaymentError::AmountTampered {
                    requested: amount,
                    approved: result.approved_amount,
                }
                .into());
            }
            record_authorization(tx, order_id, amount, currency, &result.gateway_reference).await?;
        }
        Err(e) => {
            tracing::warn!(order_id = %order_id, error = %e, "Gateway declined authorization");
            write_outbox(tx, order_id, EventType::PaymentFailed, || {
                serde_json::json!({ "order_id": order_id.to_string(), "reason": "Payment gateway declined" })
            })
            .await?;
        }
    }

    Ok(())
}

/// Capture payment: reads on pool, gateway call, writes on tx.
pub async fn capture_payment_on_tx(
    pool: &PgPool,
    tx: &mut PgConnection,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
) -> Result<(), AppError> {
    // Read phase (pool)
    let (_, gateway_ref, auth_currency, amount) =
        find_posted_authorization(pool, order_id, "capture").await?;

    // Gateway call (no DB held)
    gateway.capture(&gateway_ref).await?;

    // Write phase (tx)
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

/// Void payment: reads on pool, gateway call, writes on tx.
pub async fn void_payment_on_tx(
    pool: &PgPool,
    tx: &mut PgConnection,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
) -> Result<(), AppError> {
    // Read phase (pool)
    let (auth_tx_id, gateway_ref, auth_currency, amount) =
        find_posted_authorization(pool, order_id, "void").await?;

    // Gateway call (no DB held)
    gateway.void(&gateway_ref).await?;

    // Write phase (tx)
    let idempotency_key = format!("void:{order_id}");
    record_void(
        tx,
        order_id,
        auth_tx_id,
        &auth_currency,
        amount,
        &idempotency_key,
    )
    .await
}

// ── Shared helpers (all return AppError, used by both API styles) ──

fn validate_authorization(
    amount: Decimal,
    currency: &str,
    payment_state: &crate::ledger::value_objects::PaymentState,
) -> Result<(), AppError> {
    let auth_ctx = AuthorizationContext {
        amount,
        currency: currency.to_string(),
        payment_state: *payment_state,
    };
    let result = authorization_rules().evaluate_detailed(&eval_authorization(&auth_ctx));
    if !result.passed() {
        return Err(PaymentError::ValidationFailed(result.failure_messages().join("; ")).into());
    }
    Ok(())
}

async fn record_authorization(
    tx: &mut PgConnection,
    order_id: Uuid,
    amount: Decimal,
    currency: &str,
    gateway_ref: &str,
) -> Result<(), AppError> {
    let idempotency_key = format!("auth:{order_id}");
    let (buyer, holding) = create_account_pair(
        tx,
        order_id,
        currency,
        &AccountType::Buyer,
        &AccountType::GatewayHolding,
    )
    .await?;

    let ledger_tx = ledger_repo::create_transaction(
        tx,
        order_id,
        &TransactionType::Authorization,
        &idempotency_key,
        Some(gateway_ref),
    )
    .await?;

    write_entry_pair(tx, ledger_tx.id, holding, buyer, amount).await?;
    post_transaction(tx, ledger_tx.id).await?;
    let gw = gateway_ref.to_string();
    write_outbox(tx, order_id, EventType::PaymentAuthorized, || {
        serde_json::json!({
            "order_id": order_id.to_string(),
            "payment_id": ledger_tx.id.to_string(),
            "gateway_reference": gw,
        })
    })
    .await
}

async fn record_capture(
    tx: &mut PgConnection,
    order_id: Uuid,
    auth_currency: &str,
    amount: Decimal,
    idempotency_key: &str,
    gateway_ref: &str,
) -> Result<(), AppError> {
    let (holding, revenue) = create_account_pair(
        tx,
        order_id,
        auth_currency,
        &AccountType::GatewayHolding,
        &AccountType::PlatformRevenue,
    )
    .await?;

    let capture_tx = ledger_repo::create_transaction(
        tx,
        order_id,
        &TransactionType::Capture,
        idempotency_key,
        Some(gateway_ref),
    )
    .await?;

    write_entry_pair(tx, capture_tx.id, revenue, holding, amount).await?;
    post_transaction(tx, capture_tx.id).await?;
    write_outbox(
        tx,
        order_id,
        EventType::PaymentCaptured,
        || serde_json::json!({ "order_id": order_id.to_string() }),
    )
    .await
}

async fn record_void(
    tx: &mut PgConnection,
    order_id: Uuid,
    auth_tx_id: Uuid,
    auth_currency: &str,
    amount: Decimal,
    idempotency_key: &str,
) -> Result<(), AppError> {
    ledger_repo::update_transaction_status(tx, auth_tx_id, &TransactionStatus::Discarded, None)
        .await?;

    let (buyer, holding) = create_account_pair(
        tx,
        order_id,
        auth_currency,
        &AccountType::Buyer,
        &AccountType::GatewayHolding,
    )
    .await?;

    let void_tx = ledger_repo::create_transaction(
        tx,
        order_id,
        &TransactionType::Void,
        idempotency_key,
        None,
    )
    .await?;

    write_entry_pair(tx, void_tx.id, buyer, holding, amount).await?;
    post_transaction(tx, void_tx.id).await?;
    write_outbox(
        tx,
        order_id,
        EventType::PaymentVoided,
        || serde_json::json!({ "order_id": order_id.to_string() }),
    )
    .await
}

async fn find_posted_authorization(
    executor: &PgPool,
    order_id: Uuid,
    operation: &str,
) -> Result<(Uuid, String, String, Decimal), AppError> {
    let transactions = ledger_repo::list_transactions_by_order(executor, order_id).await?;
    let payment_state = ledger_repo::derive_payment_state(&transactions);

    match operation {
        "capture" => payment_state
            .validate_for_capture()
            .map_err(AppError::from)?,
        "void" => payment_state.validate_for_void().map_err(AppError::from)?,
        _ => {}
    }

    let auth_tx = transactions
        .iter()
        .find(|t| {
            t.transaction_type == TransactionType::Authorization
                && t.status == TransactionStatus::Posted
        })
        .ok_or(AppError::NotFound(
            "No posted authorization found".to_string(),
        ))?;

    let gateway_ref = auth_tx
        .gateway_reference
        .clone()
        .ok_or(AppError::InternalServerError(
            "Authorization missing gateway reference".to_string(),
        ))?;

    let auth_entries = ledger_repo::list_entries_by_transaction(executor, auth_tx.id).await?;
    let amount = auth_entries
        .first()
        .map(|e| e.amount)
        .unwrap_or(Decimal::ZERO);

    let currency = auth_tx.metadata["currency"]
        .as_str()
        .unwrap_or("USD")
        .to_string();

    Ok((auth_tx.id, gateway_ref, currency, amount))
}

async fn create_account_pair(
    tx: &mut PgConnection,
    order_id: Uuid,
    currency: &str,
    debit_type: &AccountType,
    credit_type: &AccountType,
) -> Result<(Uuid, Uuid), AppError> {
    let debit_acct = ledger_repo::get_or_create_account(tx, debit_type, order_id, currency).await?;
    let credit_acct =
        ledger_repo::get_or_create_account(tx, credit_type, order_id, currency).await?;
    Ok((debit_acct.id, credit_acct.id))
}

async fn write_entry_pair(
    tx: &mut PgConnection,
    transaction_id: Uuid,
    debit_account_id: Uuid,
    credit_account_id: Uuid,
    amount: Decimal,
) -> Result<(), AppError> {
    ledger_repo::create_entry(
        tx,
        transaction_id,
        debit_account_id,
        &EntryDirection::Debit,
        amount,
    )
    .await?;
    ledger_repo::create_entry(
        tx,
        transaction_id,
        credit_account_id,
        &EntryDirection::Credit,
        amount,
    )
    .await?;
    Ok(())
}

async fn post_transaction(tx: &mut PgConnection, transaction_id: Uuid) -> Result<(), AppError> {
    ledger_repo::update_transaction_status(tx, transaction_id, &TransactionStatus::Posted, None)
        .await
}

async fn write_outbox(
    tx: &mut PgConnection,
    order_id: Uuid,
    event_type: EventType,
    payload_fn: impl FnOnce() -> serde_json::Value,
) -> Result<(), AppError> {
    let metadata = EventMetadata::new(
        event_type,
        AggregateType::Payment,
        order_id,
        SourceService::Payment,
    );
    let envelope = EventEnvelope::new(metadata, payload_fn());
    let insert = OutboxInsert::from_envelope("payments.events", &envelope);
    insert_outbox_event(tx, &insert).await.map(|_| ())
}

/// Handle tampered gateway amount (pool-based path only).
async fn handle_tampered_amount(
    state: &AppState,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
    requested: Decimal,
    result: &crate::gateway::traits::GatewayAuthResult,
) -> Result<(), AppError> {
    if let Err(e) = gateway.void(&result.gateway_reference).await {
        tracing::error!(order_id = %order_id, error = %e, "Failed to void tampered authorization");
    }

    write_payment_event(state, order_id, EventType::PaymentFailed, || {
        serde_json::json!({
            "order_id": order_id.to_string(),
            "reason": format!("Amount tampering: requested={requested}, approved={}", result.approved_amount),
        })
    })
    .await?;

    Err(PaymentError::AmountTampered {
        requested,
        approved: result.approved_amount,
    }
    .into())
}

/// Write a payment event outside a transaction (creates its own).
async fn write_payment_event(
    state: &AppState,
    order_id: Uuid,
    event_type: EventType,
    payload_fn: impl FnOnce() -> serde_json::Value,
) -> Result<(), AppError> {
    let payload = payload_fn();
    let et = event_type.clone();
    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            write_outbox(tx.as_executor(), order_id, event_type, || payload).await?;
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to write {et:?}: {e}")))
}
