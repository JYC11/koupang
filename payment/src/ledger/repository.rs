use crate::ledger::entities::{
    AccountBalanceEntity, AccountEntity, LedgerEntryEntity, LedgerTransactionEntity,
};
use crate::ledger::value_objects::{
    AccountType, EntryDirection, PaymentState, TransactionStatus, TransactionType,
};
use rust_decimal::Decimal;
use shared::db::PgExec;
use shared::errors::AppError;
use sqlx::PgConnection;
use uuid::Uuid;

// ── Accounts ────────────────────────────────────────────────

pub async fn get_or_create_account(
    tx: &mut PgConnection,
    account_type: &AccountType,
    reference_id: Uuid,
    currency: &str,
) -> Result<AccountEntity, AppError> {
    let normal_balance = account_type.normal_balance();

    let account = sqlx::query_as::<_, AccountEntity>(
        "INSERT INTO accounts (account_type, normal_balance, reference_id, currency) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (reference_id, account_type, currency) DO NOTHING \
         RETURNING *",
    )
    .bind(account_type.as_str())
    .bind(normal_balance.as_str())
    .bind(reference_id)
    .bind(currency)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create account: {e}")))?;

    if let Some(account) = account {
        return Ok(account);
    }

    // Already existed — fetch it
    sqlx::query_as::<_, AccountEntity>(
        "SELECT * FROM accounts WHERE reference_id = $1 AND account_type = $2 AND currency = $3",
    )
    .bind(reference_id)
    .bind(account_type.as_str())
    .bind(currency)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to fetch account: {e}")))
}

// ── Transactions ────────────────────────────────────────────

pub async fn create_transaction(
    tx: &mut PgConnection,
    order_id: Uuid,
    transaction_type: &TransactionType,
    idempotency_key: &str,
    gateway_reference: Option<&str>,
) -> Result<LedgerTransactionEntity, AppError> {
    sqlx::query_as::<_, LedgerTransactionEntity>(
        "INSERT INTO ledger_transactions (order_id, transaction_type, idempotency_key, gateway_reference) \
         VALUES ($1, $2, $3, $4) \
         RETURNING *",
    )
    .bind(order_id)
    .bind(transaction_type.as_str())
    .bind(idempotency_key)
    .bind(gateway_reference)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create transaction: {e}")))
}

pub async fn get_transaction_by_idempotency_key<'e>(
    executor: impl PgExec<'e>,
    key: &str,
) -> Result<Option<LedgerTransactionEntity>, AppError> {
    sqlx::query_as::<_, LedgerTransactionEntity>(
        "SELECT * FROM ledger_transactions WHERE idempotency_key = $1",
    )
    .bind(key)
    .fetch_optional(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to fetch transaction: {e}")))
}

pub async fn update_transaction_status(
    tx: &mut PgConnection,
    id: Uuid,
    status: &TransactionStatus,
    gateway_reference: Option<&str>,
) -> Result<(), AppError> {
    let result = if let Some(gw_ref) = gateway_reference {
        sqlx::query(
            "UPDATE ledger_transactions SET status = $1, gateway_reference = $2 WHERE id = $3",
        )
        .bind(status.as_str())
        .bind(gw_ref)
        .bind(id)
        .execute(&mut *tx)
        .await
    } else {
        sqlx::query("UPDATE ledger_transactions SET status = $1 WHERE id = $2")
            .bind(status.as_str())
            .bind(id)
            .execute(&mut *tx)
            .await
    };

    let result = result
        .map_err(|e| AppError::InternalServerError(format!("Failed to update transaction: {e}")))?;

    assert_eq!(
        result.rows_affected(),
        1,
        "UPDATE must affect exactly 1 row"
    );
    Ok(())
}

pub async fn list_transactions_by_order<'e>(
    executor: impl PgExec<'e>,
    order_id: Uuid,
) -> Result<Vec<LedgerTransactionEntity>, AppError> {
    sqlx::query_as::<_, LedgerTransactionEntity>(
        "SELECT * FROM ledger_transactions WHERE order_id = $1 ORDER BY created_at DESC",
    )
    .bind(order_id)
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to list transactions: {e}")))
}

// ── Entries ─────────────────────────────────────────────────

pub async fn create_entry(
    tx: &mut PgConnection,
    transaction_id: Uuid,
    account_id: Uuid,
    direction: &EntryDirection,
    amount: Decimal,
) -> Result<LedgerEntryEntity, AppError> {
    sqlx::query_as::<_, LedgerEntryEntity>(
        "INSERT INTO ledger_entries (transaction_id, account_id, direction, amount) \
         VALUES ($1, $2, $3, $4) \
         RETURNING *",
    )
    .bind(transaction_id)
    .bind(account_id)
    .bind(direction.as_str())
    .bind(amount)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create entry: {e}")))
}

pub async fn list_entries_by_transaction<'e>(
    executor: impl PgExec<'e>,
    transaction_id: Uuid,
) -> Result<Vec<LedgerEntryEntity>, AppError> {
    sqlx::query_as::<_, LedgerEntryEntity>(
        "SELECT * FROM ledger_entries WHERE transaction_id = $1 ORDER BY created_at ASC",
    )
    .bind(transaction_id)
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to list entries: {e}")))
}

// ── Balances ────────────────────────────────────────────────

pub async fn get_account_balances<'e>(
    executor: impl PgExec<'e>,
    reference_id: Uuid,
) -> Result<Vec<AccountBalanceEntity>, AppError> {
    sqlx::query_as::<_, AccountBalanceEntity>(
        "SELECT * FROM account_balances WHERE reference_id = $1",
    )
    .bind(reference_id)
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to fetch balances: {e}")))
}

// ── Derive payment state ────────────────────────────────────

pub fn derive_payment_state(transactions: &[LedgerTransactionEntity]) -> PaymentState {
    if transactions.is_empty() {
        return PaymentState::New;
    }

    // Check posted transactions in reverse chronological order
    for tx in transactions {
        if tx.status == TransactionStatus::Posted {
            return match tx.transaction_type {
                TransactionType::Refund => PaymentState::Refunded,
                TransactionType::Void => PaymentState::Voided,
                TransactionType::Capture => PaymentState::Captured,
                TransactionType::Authorization => PaymentState::Authorized,
            };
        }
    }

    // Check for pending transactions
    for tx in transactions {
        if tx.status == TransactionStatus::Pending {
            return PaymentState::Pending;
        }
    }

    // All discarded
    PaymentState::Failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::entities::LedgerTransactionEntity;
    use chrono::Utc;

    fn make_tx(tx_type: TransactionType, status: TransactionStatus) -> LedgerTransactionEntity {
        LedgerTransactionEntity {
            id: Uuid::now_v7(),
            created_at: Utc::now(),
            order_id: Uuid::now_v7(),
            transaction_type: tx_type,
            status,
            idempotency_key: "test".to_string(),
            gateway_reference: None,
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn empty_transactions_is_new() {
        assert_eq!(derive_payment_state(&[]), PaymentState::New);
    }

    #[test]
    fn posted_authorization_is_authorized() {
        let txs = vec![make_tx(
            TransactionType::Authorization,
            TransactionStatus::Posted,
        )];
        assert_eq!(derive_payment_state(&txs), PaymentState::Authorized);
    }

    #[test]
    fn posted_capture_is_captured() {
        let txs = vec![
            make_tx(TransactionType::Capture, TransactionStatus::Posted),
            make_tx(TransactionType::Authorization, TransactionStatus::Posted),
        ];
        assert_eq!(derive_payment_state(&txs), PaymentState::Captured);
    }

    #[test]
    fn posted_void_is_voided() {
        let txs = vec![
            make_tx(TransactionType::Void, TransactionStatus::Posted),
            make_tx(TransactionType::Authorization, TransactionStatus::Discarded),
        ];
        assert_eq!(derive_payment_state(&txs), PaymentState::Voided);
    }

    #[test]
    fn posted_refund_is_refunded() {
        let txs = vec![
            make_tx(TransactionType::Refund, TransactionStatus::Posted),
            make_tx(TransactionType::Capture, TransactionStatus::Posted),
        ];
        assert_eq!(derive_payment_state(&txs), PaymentState::Refunded);
    }

    #[test]
    fn pending_only_is_pending() {
        let txs = vec![make_tx(
            TransactionType::Authorization,
            TransactionStatus::Pending,
        )];
        assert_eq!(derive_payment_state(&txs), PaymentState::Pending);
    }

    #[test]
    fn all_discarded_is_failed() {
        let txs = vec![make_tx(
            TransactionType::Authorization,
            TransactionStatus::Discarded,
        )];
        assert_eq!(derive_payment_state(&txs), PaymentState::Failed);
    }
}
