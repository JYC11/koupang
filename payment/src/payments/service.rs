use crate::AppState;
use crate::gateway::traits::PaymentGateway;
use crate::ledger::repository as ledger_repo;
use crate::ledger::value_objects::{
    AccountType, EntryDirection, TransactionStatus, TransactionType,
};
use crate::payments::error::PaymentError;
use crate::payments::rules::{AuthorizationContext, authorization_rules, eval_authorization};
use rust_decimal::Decimal;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::errors::AppError;
use shared::events::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};
use shared::outbox::{OutboxInsert, insert_outbox_event};
use sqlx::PgConnection;
use uuid::Uuid;

/// Authorize payment for an order. Called when InventoryReserved is received.
pub async fn authorize_payment(
    state: &AppState,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
    amount: Decimal,
    currency: &str,
) -> Result<(), AppError> {
    let idempotency_key = format!("auth:{order_id}");

    // 3-case idempotency: check if transaction already exists.
    let existing_tx =
        ledger_repo::get_transaction_by_idempotency_key(&state.pool, &idempotency_key).await?;

    if let Some(ref existing) = existing_tx {
        if existing.status != TransactionStatus::Discarded {
            return Ok(()); // Case 1: already processed, safe retry.
        }
        // Case 3: previous attempt failed, allow retry.
    }

    // Evaluate authorization rules
    let transactions = ledger_repo::list_transactions_by_order(&state.pool, order_id).await?;
    let payment_state = ledger_repo::derive_payment_state(&transactions);
    let auth_ctx = AuthorizationContext {
        amount,
        currency: currency.to_string(),
        payment_state,
    };
    let result = authorization_rules().evaluate_detailed(&eval_authorization(&auth_ctx));
    if !result.passed() {
        return Err(PaymentError::ValidationFailed(result.failure_messages().join("; ")).into());
    }

    let auth_result = gateway
        .authorize(&idempotency_key, order_id, amount, currency)
        .await;

    match auth_result {
        Ok(result) => {
            if result.approved_amount != amount {
                return handle_tampered_amount(state, gateway, order_id, amount, &result).await;
            }
            record_authorization(state, order_id, amount, currency, &result.gateway_reference)
                .await?;
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

/// Capture an authorized payment. Called when OrderConfirmed is received.
pub async fn capture_payment(
    state: &AppState,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
) -> Result<(), AppError> {
    let (auth_tx_id, gateway_ref, auth_currency, amount) =
        find_posted_authorization(state, order_id, "capture").await?;

    let _capture_result = gateway.capture(&gateway_ref).await?;

    let idempotency_key = format!("capture:{order_id}");

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            let (holding, revenue) = create_account_pair(
                tx.as_executor(),
                order_id,
                &auth_currency,
                &AccountType::GatewayHolding,
                &AccountType::PlatformRevenue,
            )
            .await?;

            let capture_tx = ledger_repo::create_transaction(
                tx.as_executor(),
                order_id,
                &TransactionType::Capture,
                &idempotency_key,
                Some(&gateway_ref),
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            // Debit platform_revenue, Credit gateway_holding.
            write_entry_pair(tx.as_executor(), capture_tx.id, revenue, holding, amount).await?;
            post_transaction(tx.as_executor(), capture_tx.id).await?;
            write_outbox(
                tx.as_executor(),
                order_id,
                EventType::PaymentCaptured,
                || serde_json::json!({ "order_id": order_id.to_string() }),
            )
            .await?;

            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to capture payment: {e}")))
}

/// Void an authorized payment. Called when OrderCancelled is received (pre-capture).
pub async fn void_payment(
    state: &AppState,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
) -> Result<(), AppError> {
    let (auth_tx_id, gateway_ref, auth_currency, amount) =
        find_posted_authorization(state, order_id, "void").await?;

    gateway.void(&gateway_ref).await?;

    let idempotency_key = format!("void:{order_id}");

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            // Discard the original authorization.
            ledger_repo::update_transaction_status(
                tx.as_executor(),
                auth_tx_id,
                &TransactionStatus::Discarded,
                None,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            let (buyer, holding) = create_account_pair(
                tx.as_executor(),
                order_id,
                &auth_currency,
                &AccountType::Buyer,
                &AccountType::GatewayHolding,
            )
            .await?;

            let void_tx = ledger_repo::create_transaction(
                tx.as_executor(),
                order_id,
                &TransactionType::Void,
                &idempotency_key,
                None,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            // Debit buyer (return money), Credit gateway_holding.
            write_entry_pair(tx.as_executor(), void_tx.id, buyer, holding, amount).await?;
            post_transaction(tx.as_executor(), void_tx.id).await?;
            write_outbox(
                tx.as_executor(),
                order_id,
                EventType::PaymentVoided,
                || serde_json::json!({ "order_id": order_id.to_string() }),
            )
            .await?;

            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to void payment: {e}")))
}

// ── Helpers (extracted to meet 70-line limit) ───────────────

/// Handle tampered gateway amount: void the auth and write PaymentFailed.
async fn handle_tampered_amount(
    state: &AppState,
    gateway: &dyn PaymentGateway,
    order_id: Uuid,
    requested: Decimal,
    result: &crate::gateway::traits::GatewayAuthResult,
) -> Result<(), AppError> {
    // Best-effort void — log failure but don't block the tamper error.
    if let Err(e) = gateway.void(&result.gateway_reference).await {
        tracing::error!(order_id = %order_id, error = %e, "Failed to void tampered authorization");
    }

    write_payment_event(
        state,
        order_id,
        EventType::PaymentFailed,
        || {
            serde_json::json!({
                "order_id": order_id.to_string(),
                "reason": format!("Amount tampering: requested={requested}, approved={}", result.approved_amount),
            })
        },
    )
    .await?;

    Err(PaymentError::AmountTampered {
        requested,
        approved: result.approved_amount,
    }
    .into())
}

/// Record a successful authorization: create accounts, transaction, entries, outbox.
async fn record_authorization(
    state: &AppState,
    order_id: Uuid,
    amount: Decimal,
    currency: &str,
    gateway_ref: &str,
) -> Result<(), AppError> {
    let currency_owned = currency.to_string();
    let gateway_ref_owned = gateway_ref.to_string();
    let idempotency_key = format!("auth:{order_id}");

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            let (buyer, holding) = create_account_pair(
                tx.as_executor(),
                order_id,
                &currency_owned,
                &AccountType::Buyer,
                &AccountType::GatewayHolding,
            )
            .await?;

            let ledger_tx = ledger_repo::create_transaction(
                tx.as_executor(),
                order_id,
                &TransactionType::Authorization,
                &idempotency_key,
                Some(&gateway_ref_owned),
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            // Debit gateway_holding, Credit buyer.
            write_entry_pair(tx.as_executor(), ledger_tx.id, holding, buyer, amount).await?;
            post_transaction(tx.as_executor(), ledger_tx.id).await?;
            write_outbox(
                tx.as_executor(),
                order_id,
                EventType::PaymentAuthorized,
                || {
                    serde_json::json!({
                        "order_id": order_id.to_string(),
                        "payment_id": ledger_tx.id.to_string(),
                        "gateway_reference": gateway_ref_owned,
                    })
                },
            )
            .await?;

            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to authorize payment: {e}")))
}

/// Find the posted authorization for an order. Returns (tx_id, gateway_ref, currency, amount).
async fn find_posted_authorization(
    state: &AppState,
    order_id: Uuid,
    operation: &str,
) -> Result<(Uuid, String, String, Decimal), AppError> {
    let transactions = ledger_repo::list_transactions_by_order(&state.pool, order_id).await?;
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

    let auth_entries = ledger_repo::list_entries_by_transaction(&state.pool, auth_tx.id).await?;
    let amount = auth_entries
        .first()
        .map(|e| e.amount)
        .unwrap_or(Decimal::ZERO);

    // Currency from the account associated with the first entry.
    let currency = auth_tx.metadata["currency"]
        .as_str()
        .unwrap_or("USD")
        .to_string();

    Ok((auth_tx.id, gateway_ref, currency, amount))
}

/// Create or fetch two accounts in one go. Returns (debit_account_id, credit_account_id).
async fn create_account_pair(
    tx: &mut PgConnection,
    order_id: Uuid,
    currency: &str,
    debit_type: &AccountType,
    credit_type: &AccountType,
) -> Result<(Uuid, Uuid), TxError> {
    let debit_acct = ledger_repo::get_or_create_account(tx, debit_type, order_id, currency)
        .await
        .map_err(|e| TxError::Other(e.to_string()))?;
    let credit_acct = ledger_repo::get_or_create_account(tx, credit_type, order_id, currency)
        .await
        .map_err(|e| TxError::Other(e.to_string()))?;
    Ok((debit_acct.id, credit_acct.id))
}

/// Write a debit + credit entry pair for a transaction.
async fn write_entry_pair(
    tx: &mut PgConnection,
    transaction_id: Uuid,
    debit_account_id: Uuid,
    credit_account_id: Uuid,
    amount: Decimal,
) -> Result<(), TxError> {
    ledger_repo::create_entry(
        tx,
        transaction_id,
        debit_account_id,
        &EntryDirection::Debit,
        amount,
    )
    .await
    .map_err(|e| TxError::Other(e.to_string()))?;
    ledger_repo::create_entry(
        tx,
        transaction_id,
        credit_account_id,
        &EntryDirection::Credit,
        amount,
    )
    .await
    .map_err(|e| TxError::Other(e.to_string()))?;
    Ok(())
}

/// Mark a ledger transaction as posted.
async fn post_transaction(tx: &mut PgConnection, transaction_id: Uuid) -> Result<(), TxError> {
    ledger_repo::update_transaction_status(tx, transaction_id, &TransactionStatus::Posted, None)
        .await
        .map_err(|e| TxError::Other(e.to_string()))
}

/// Write an outbox event inside a transaction.
async fn write_outbox(
    tx: &mut PgConnection,
    order_id: Uuid,
    event_type: EventType,
    payload_fn: impl FnOnce() -> serde_json::Value,
) -> Result<(), TxError> {
    let metadata = EventMetadata::new(
        event_type,
        AggregateType::Payment,
        order_id,
        SourceService::Payment,
    );
    let envelope = EventEnvelope::new(metadata, payload_fn());
    let insert = OutboxInsert::from_envelope("payments.events", &envelope);
    insert_outbox_event(tx, &insert)
        .await
        .map(|_| ())
        .map_err(|e| TxError::Other(e.to_string()))
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
