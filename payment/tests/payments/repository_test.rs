use payment::ledger::repository;
use payment::ledger::value_objects::{
    AccountType, EntryDirection, PaymentState, TransactionStatus, TransactionType,
};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::common::test_db;

// ── Accounts ────────────────────────────────────────────────

#[tokio::test]
async fn get_or_create_account_creates_new_account() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let account =
        repository::get_or_create_account(&mut *conn, &AccountType::Buyer, order_id, "USD")
            .await
            .unwrap();

    assert_eq!(account.account_type, AccountType::Buyer);
    assert_eq!(account.reference_id, order_id);
    assert_eq!(account.currency, "USD");
}

#[tokio::test]
async fn get_or_create_account_returns_existing_on_conflict() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let a1 = repository::get_or_create_account(&mut *conn, &AccountType::Buyer, order_id, "USD")
        .await
        .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    let a2 = repository::get_or_create_account(&mut *conn2, &AccountType::Buyer, order_id, "USD")
        .await
        .unwrap();

    assert_eq!(a1.id, a2.id, "should return same account");
}

#[tokio::test]
async fn different_account_types_for_same_order() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let buyer = repository::get_or_create_account(&mut *conn, &AccountType::Buyer, order_id, "USD")
        .await
        .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    let holding = repository::get_or_create_account(
        &mut *conn2,
        &AccountType::GatewayHolding,
        order_id,
        "USD",
    )
    .await
    .unwrap();

    assert_ne!(buyer.id, holding.id);
    assert_eq!(buyer.reference_id, holding.reference_id);
}

// ── Transactions ────────────────────────────────────────────

#[tokio::test]
async fn create_transaction_returns_pending() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let tx = repository::create_transaction(
        &mut *conn,
        order_id,
        &TransactionType::Authorization,
        "auth:test-1",
        Some("gw-ref-123"),
    )
    .await
    .unwrap();

    assert_eq!(tx.order_id, order_id);
    assert_eq!(tx.transaction_type, TransactionType::Authorization);
    assert_eq!(tx.status, TransactionStatus::Pending);
    assert_eq!(tx.idempotency_key, "auth:test-1");
    assert_eq!(tx.gateway_reference.as_deref(), Some("gw-ref-123"));
}

#[tokio::test]
async fn get_transaction_by_idempotency_key() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let created = repository::create_transaction(
        &mut *conn,
        order_id,
        &TransactionType::Authorization,
        "auth:idem-key-1",
        None,
    )
    .await
    .unwrap();

    let found = repository::get_transaction_by_idempotency_key(&db.pool, "auth:idem-key-1")
        .await
        .unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, created.id);

    let not_found = repository::get_transaction_by_idempotency_key(&db.pool, "nonexistent")
        .await
        .unwrap();
    assert!(not_found.is_none());
}

#[tokio::test]
async fn duplicate_idempotency_key_fails() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::create_transaction(
        &mut *conn,
        order_id,
        &TransactionType::Authorization,
        "auth:dup-1",
        None,
    )
    .await
    .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    let result = repository::create_transaction(
        &mut *conn2,
        order_id,
        &TransactionType::Capture,
        "auth:dup-1",
        None,
    )
    .await;

    assert!(result.is_err(), "duplicate idempotency_key should fail");
}

#[tokio::test]
async fn update_transaction_status_to_posted() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let tx = repository::create_transaction(
        &mut *conn,
        order_id,
        &TransactionType::Authorization,
        "auth:update-1",
        None,
    )
    .await
    .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    repository::update_transaction_status(&mut *conn2, tx.id, &TransactionStatus::Posted, None)
        .await
        .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(txs[0].status, TransactionStatus::Posted);
}

#[tokio::test]
async fn list_transactions_by_order_ordered_desc() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::create_transaction(
        &mut *conn,
        order_id,
        &TransactionType::Authorization,
        "auth:list-1",
        None,
    )
    .await
    .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    repository::create_transaction(
        &mut *conn2,
        order_id,
        &TransactionType::Capture,
        "capture:list-1",
        None,
    )
    .await
    .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(txs.len(), 2);
    // DESC order — capture (newer) first
    assert_eq!(txs[0].transaction_type, TransactionType::Capture);
    assert_eq!(txs[1].transaction_type, TransactionType::Authorization);
}

// ── Entries ─────────────────────────────────────────────────

#[tokio::test]
async fn create_entry_pair_and_list() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();
    let amount = Decimal::new(5000, 2); // $50.00

    // Reuse a single connection to avoid pool exhaustion
    let mut conn = db.pool.acquire().await.unwrap();
    let buyer = repository::get_or_create_account(&mut *conn, &AccountType::Buyer, order_id, "USD")
        .await
        .unwrap();
    let holding = repository::get_or_create_account(
        &mut *conn,
        &AccountType::GatewayHolding,
        order_id,
        "USD",
    )
    .await
    .unwrap();
    let tx = repository::create_transaction(
        &mut *conn,
        order_id,
        &TransactionType::Authorization,
        "auth:entry-1",
        None,
    )
    .await
    .unwrap();
    repository::create_entry(
        &mut *conn,
        tx.id,
        holding.id,
        &EntryDirection::Debit,
        amount,
    )
    .await
    .unwrap();
    repository::create_entry(&mut *conn, tx.id, buyer.id, &EntryDirection::Credit, amount)
        .await
        .unwrap();
    drop(conn);

    let entries = repository::list_entries_by_transaction(&db.pool, tx.id)
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);

    let debit = entries
        .iter()
        .find(|e| e.direction == EntryDirection::Debit)
        .unwrap();
    let credit = entries
        .iter()
        .find(|e| e.direction == EntryDirection::Credit)
        .unwrap();
    assert_eq!(debit.amount, amount);
    assert_eq!(credit.amount, amount);
    assert_eq!(debit.account_id, holding.id);
    assert_eq!(credit.account_id, buyer.id);
}

// ── Balances view ───────────────────────────────────────────

#[tokio::test]
async fn account_balances_reflect_posted_entries() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();
    let amount = Decimal::new(5000, 2); // $50.00

    // Reuse a single connection to avoid pool exhaustion
    let mut conn = db.pool.acquire().await.unwrap();
    let buyer = repository::get_or_create_account(&mut *conn, &AccountType::Buyer, order_id, "USD")
        .await
        .unwrap();
    let holding = repository::get_or_create_account(
        &mut *conn,
        &AccountType::GatewayHolding,
        order_id,
        "USD",
    )
    .await
    .unwrap();
    let tx = repository::create_transaction(
        &mut *conn,
        order_id,
        &TransactionType::Authorization,
        "auth:bal-1",
        None,
    )
    .await
    .unwrap();
    repository::create_entry(
        &mut *conn,
        tx.id,
        holding.id,
        &EntryDirection::Debit,
        amount,
    )
    .await
    .unwrap();
    repository::create_entry(&mut *conn, tx.id, buyer.id, &EntryDirection::Credit, amount)
        .await
        .unwrap();
    repository::update_transaction_status(&mut *conn, tx.id, &TransactionStatus::Posted, None)
        .await
        .unwrap();
    drop(conn);

    let balances = repository::get_account_balances(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(balances.len(), 2);

    let buyer_bal = balances
        .iter()
        .find(|b| b.account_type == AccountType::Buyer)
        .unwrap();
    let holding_bal = balances
        .iter()
        .find(|b| b.account_type == AccountType::GatewayHolding)
        .unwrap();

    // Buyer has debit normal balance, credit entry → negative balance
    assert_eq!(buyer_bal.total_credits, amount);
    // GatewayHolding has debit normal balance, debit entry → positive balance
    assert_eq!(holding_bal.total_debits, amount);
    assert_eq!(holding_bal.balance, amount);
}

#[tokio::test]
async fn pending_entries_not_in_balances() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let buyer = repository::get_or_create_account(&mut *conn, &AccountType::Buyer, order_id, "USD")
        .await
        .unwrap();
    let tx = repository::create_transaction(
        &mut *conn,
        order_id,
        &TransactionType::Authorization,
        "auth:pending-bal",
        None,
    )
    .await
    .unwrap();
    repository::create_entry(
        &mut *conn,
        tx.id,
        buyer.id,
        &EntryDirection::Debit,
        Decimal::new(100, 0),
    )
    .await
    .unwrap();
    drop(conn);

    // Don't post the transaction — balance should be zero
    let balances = repository::get_account_balances(&db.pool, order_id)
        .await
        .unwrap();
    let buyer_bal = balances
        .iter()
        .find(|b| b.account_type == AccountType::Buyer)
        .unwrap();
    assert_eq!(buyer_bal.balance, Decimal::ZERO);
}

// ── Derive payment state ────────────────────────────────────

#[tokio::test]
async fn derive_state_from_real_transactions() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    // No transactions → New
    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(repository::derive_payment_state(&txs), PaymentState::New);

    // Create and post an authorization → Authorized
    let mut conn = db.pool.acquire().await.unwrap();
    let auth_tx = repository::create_transaction(
        &mut *conn,
        order_id,
        &TransactionType::Authorization,
        "auth:state-1",
        Some("gw-123"),
    )
    .await
    .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    repository::update_transaction_status(
        &mut *conn2,
        auth_tx.id,
        &TransactionStatus::Posted,
        None,
    )
    .await
    .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(
        repository::derive_payment_state(&txs),
        PaymentState::Authorized
    );
}
