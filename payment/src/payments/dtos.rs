use crate::ledger::entities::{AccountBalanceEntity, LedgerEntryEntity, LedgerTransactionEntity};
use crate::ledger::value_objects::{
    AccountType, EntryDirection, NormalBalance, PaymentState, TransactionStatus, TransactionType,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use shared::dto_helpers::{fmt_datetime, fmt_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentStatusRes {
    pub order_id: String,
    pub state: PaymentState,
    pub transactions: Vec<TransactionRes>,
    pub balances: Vec<AccountBalanceRes>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRes {
    pub id: String,
    pub created_at: String,
    pub transaction_type: TransactionType,
    pub status: TransactionStatus,
    pub idempotency_key: String,
    pub gateway_reference: Option<String>,
    pub entries: Vec<EntryRes>,
}

impl TransactionRes {
    pub fn new(entity: LedgerTransactionEntity, entries: Vec<LedgerEntryEntity>) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            transaction_type: entity.transaction_type,
            status: entity.status,
            idempotency_key: entity.idempotency_key,
            gateway_reference: entity.gateway_reference,
            entries: entries.into_iter().map(EntryRes::new).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryRes {
    pub id: String,
    pub account_id: String,
    pub direction: EntryDirection,
    pub amount: Decimal,
}

impl EntryRes {
    pub fn new(entity: LedgerEntryEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            account_id: fmt_id(&entity.account_id),
            direction: entity.direction,
            amount: entity.amount,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalanceRes {
    pub account_type: AccountType,
    pub normal_balance: NormalBalance,
    pub currency: String,
    pub total_debits: Decimal,
    pub total_credits: Decimal,
    pub balance: Decimal,
}

impl AccountBalanceRes {
    pub fn new(entity: AccountBalanceEntity) -> Self {
        Self {
            account_type: entity.account_type,
            normal_balance: entity.normal_balance,
            currency: entity.currency,
            total_debits: entity.total_debits,
            total_credits: entity.total_credits,
            balance: entity.balance,
        }
    }
}
