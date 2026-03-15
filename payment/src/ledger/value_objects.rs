use crate::payments::error::PaymentError;
use serde::{Deserialize, Serialize};
use std::fmt;

// ── AccountType ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum AccountType {
    Buyer,
    GatewayHolding,
    PlatformRevenue,
    SellerPayable,
}

impl AccountType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Buyer => "buyer",
            Self::GatewayHolding => "gateway_holding",
            Self::PlatformRevenue => "platform_revenue",
            Self::SellerPayable => "seller_payable",
        }
    }

    pub fn normal_balance(&self) -> NormalBalance {
        match self {
            Self::Buyer | Self::GatewayHolding => NormalBalance::Debit,
            Self::PlatformRevenue | Self::SellerPayable => NormalBalance::Credit,
        }
    }
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── NormalBalance ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum NormalBalance {
    Debit,
    Credit,
}

impl NormalBalance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debit => "debit",
            Self::Credit => "credit",
        }
    }
}

impl fmt::Display for NormalBalance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── TransactionType ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum TransactionType {
    Authorization,
    Capture,
    Void,
    Refund,
}

impl TransactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Authorization => "authorization",
            Self::Capture => "capture",
            Self::Void => "void",
            Self::Refund => "refund",
        }
    }
}

impl fmt::Display for TransactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── TransactionStatus ──────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum TransactionStatus {
    Pending,
    Posted,
    Discarded,
}

impl TransactionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Posted => "posted",
            Self::Discarded => "discarded",
        }
    }
}

impl fmt::Display for TransactionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── EntryDirection ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum EntryDirection {
    Debit,
    Credit,
}

impl EntryDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debit => "debit",
            Self::Credit => "credit",
        }
    }
}

impl fmt::Display for EntryDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── PaymentState (derived from ledger transactions) ────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentState {
    New,
    Authorized,
    Captured,
    Voided,
    Refunded,
    Pending,
    Failed,
}

impl PaymentState {
    pub fn validate_for_authorize(&self) -> Result<(), PaymentError> {
        match self {
            Self::New | Self::Failed => Ok(()),
            state => Err(PaymentError::InvalidState {
                operation: "authorize".to_string(),
                state: *state,
            }),
        }
    }

    pub fn validate_for_capture(&self) -> Result<(), PaymentError> {
        match self {
            Self::Authorized => Ok(()),
            state => Err(PaymentError::InvalidState {
                operation: "capture".to_string(),
                state: *state,
            }),
        }
    }

    pub fn validate_for_void(&self) -> Result<(), PaymentError> {
        match self {
            Self::Authorized => Ok(()),
            state => Err(PaymentError::InvalidState {
                operation: "void".to_string(),
                state: *state,
            }),
        }
    }

    pub fn validate_for_refund(&self) -> Result<(), PaymentError> {
        match self {
            Self::Captured => Ok(()),
            state => Err(PaymentError::InvalidState {
                operation: "refund".to_string(),
                state: *state,
            }),
        }
    }
}

impl fmt::Display for PaymentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ── Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_type_normal_balances() {
        assert_eq!(AccountType::Buyer.normal_balance(), NormalBalance::Debit);
        assert_eq!(
            AccountType::GatewayHolding.normal_balance(),
            NormalBalance::Debit
        );
        assert_eq!(
            AccountType::PlatformRevenue.normal_balance(),
            NormalBalance::Credit
        );
        assert_eq!(
            AccountType::SellerPayable.normal_balance(),
            NormalBalance::Credit
        );
    }

    #[test]
    fn account_type_as_str() {
        assert_eq!(AccountType::Buyer.as_str(), "buyer");
        assert_eq!(AccountType::GatewayHolding.as_str(), "gateway_holding");
        assert_eq!(AccountType::PlatformRevenue.as_str(), "platform_revenue");
        assert_eq!(AccountType::SellerPayable.as_str(), "seller_payable");
    }

    #[test]
    fn transaction_type_as_str() {
        assert_eq!(TransactionType::Authorization.as_str(), "authorization");
        assert_eq!(TransactionType::Capture.as_str(), "capture");
        assert_eq!(TransactionType::Void.as_str(), "void");
        assert_eq!(TransactionType::Refund.as_str(), "refund");
    }

    #[test]
    fn transaction_status_as_str() {
        assert_eq!(TransactionStatus::Pending.as_str(), "pending");
        assert_eq!(TransactionStatus::Posted.as_str(), "posted");
        assert_eq!(TransactionStatus::Discarded.as_str(), "discarded");
    }

    #[test]
    fn entry_direction_as_str() {
        assert_eq!(EntryDirection::Debit.as_str(), "debit");
        assert_eq!(EntryDirection::Credit.as_str(), "credit");
    }

    // ── PaymentState validation ──────────────────────────────

    #[test]
    fn authorize_allowed_from_new_and_failed() {
        assert!(PaymentState::New.validate_for_authorize().is_ok());
        assert!(PaymentState::Failed.validate_for_authorize().is_ok());
    }

    #[test]
    fn authorize_rejected_from_other_states() {
        assert!(PaymentState::Authorized.validate_for_authorize().is_err());
        assert!(PaymentState::Captured.validate_for_authorize().is_err());
        assert!(PaymentState::Voided.validate_for_authorize().is_err());
    }

    #[test]
    fn capture_only_from_authorized() {
        assert!(PaymentState::Authorized.validate_for_capture().is_ok());
        assert!(PaymentState::New.validate_for_capture().is_err());
        assert!(PaymentState::Captured.validate_for_capture().is_err());
    }

    #[test]
    fn void_only_from_authorized() {
        assert!(PaymentState::Authorized.validate_for_void().is_ok());
        assert!(PaymentState::Captured.validate_for_void().is_err());
    }

    #[test]
    fn refund_only_from_captured() {
        assert!(PaymentState::Captured.validate_for_refund().is_ok());
        assert!(PaymentState::Authorized.validate_for_refund().is_err());
    }
}
