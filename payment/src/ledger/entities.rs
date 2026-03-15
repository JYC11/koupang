use rust_decimal::Decimal;
use sqlx::FromRow;
use sqlx::types::Uuid;
use sqlx::types::chrono::{DateTime, Utc};

use super::value_objects::{
    AccountType, EntryDirection, NormalBalance, TransactionStatus, TransactionType,
};

#[derive(Debug, Clone, FromRow)]
pub struct AccountEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub account_type: AccountType,
    pub normal_balance: NormalBalance,
    pub reference_id: Uuid,
    pub currency: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct LedgerTransactionEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub order_id: Uuid,
    pub transaction_type: TransactionType,
    pub status: TransactionStatus,
    pub idempotency_key: String,
    pub gateway_reference: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, FromRow)]
pub struct LedgerEntryEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub transaction_id: Uuid,
    pub account_id: Uuid,
    pub direction: EntryDirection,
    pub amount: Decimal,
}

#[derive(Debug, Clone, FromRow)]
pub struct AccountBalanceEntity {
    pub account_id: Uuid,
    pub account_type: AccountType,
    pub reference_id: Uuid,
    pub normal_balance: NormalBalance,
    pub currency: String,
    pub total_debits: Decimal,
    pub total_credits: Decimal,
    pub balance: Decimal,
}
