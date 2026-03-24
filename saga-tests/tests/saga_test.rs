mod common;

use common::SagaHarness;

use catalog::test_fixtures::create_product_with_sku;
use order::AppState;
use order::orders::repository as order_repo;
use order::orders::value_objects::{OrderId, OrderStatus};
use order::test_fixtures::{create_order_via_service, sample_order_item_with_sku};
use payment::gateway::mock::MockPaymentGateway;
use payment::ledger::repository as ledger_repo;
use payment::ledger::value_objects::PaymentState;
use shared::auth::Role;
use shared::auth::jwt::CurrentUser;
use shared::events::EventType;
use shared::test_utils::auth::test_auth_config;
use shared::test_utils::events::make_envelope;

// ── Scenario 1: Happy Path ──────────────────────────────────────────
//
// OrderCreated → InventoryReserved → PaymentAuthorized → OrderConfirmed → PaymentCaptured
// Final: Order=Confirmed, Inventory reserved, Payment=Captured

#[tokio::test]
async fn saga_happy_path() {
    let h = SagaHarness::start(MockPaymentGateway::always_succeeds()).await;

    // Seed: catalog product + SKU with stock, order referencing that SKU.
    let (_, sku_id, seller_id) = create_product_with_sku(h.catalog_pool(), 100).await;
    let items = vec![sample_order_item_with_sku(seller_id, sku_id, 2)];
    let (_order_id, _buyer_id) = create_order_via_service(h.order_pool(), items).await;

    // Step 1: Drain OrderCreated from order outbox.
    let order_created = h.drain_outbox(h.order_pool(), "OrderCreated").await;
    let order_id = order_created.payload_uuid("order_id").unwrap();

    // Step 2: Deliver to catalog → reserves inventory.
    h.deliver_to_catalog(&order_created).await.unwrap();

    // Step 3: Drain InventoryReserved from catalog outbox.
    let inv_reserved = h.drain_outbox(h.catalog_pool(), "InventoryReserved").await;

    // Step 4: Deliver to order → transitions to InventoryReserved.
    h.deliver_to_order(&inv_reserved).await;

    // Step 5: Deliver same event to payment → authorizes payment (fan-out).
    h.deliver_to_payment(&inv_reserved).await;

    // Step 6: Drain PaymentAuthorized from payment outbox.
    let pay_authorized = h.drain_outbox(h.payment_pool(), "PaymentAuthorized").await;

    // Step 7: Deliver to order → auto-confirms (PaymentAuthorized → Confirmed).
    h.deliver_to_order(&pay_authorized).await;

    // Step 8: Drain OrderConfirmed from order outbox.
    let order_confirmed = h.drain_outbox(h.order_pool(), "OrderConfirmed").await;

    // Step 9: Deliver to payment → captures payment.
    h.deliver_to_payment(&order_confirmed).await;

    // ── Assertions ──────────────────────────────────────────────
    // Order: Confirmed
    let order = order_repo::get_order_by_id(h.order_pool(), OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Confirmed);

    // Catalog: reserved_quantity = 2
    let reserved: (i32,) = sqlx::query_as("SELECT reserved_quantity FROM skus WHERE id = $1")
        .bind(sku_id)
        .fetch_one(h.catalog_pool())
        .await
        .unwrap();
    assert_eq!(reserved.0, 2);

    // Payment: Captured
    let txs = ledger_repo::list_transactions_by_order(h.payment_pool(), order_id)
        .await
        .unwrap();
    assert_eq!(
        ledger_repo::derive_payment_state(&txs),
        PaymentState::Captured
    );

    // PaymentCaptured written to outbox.
    h.drain_outbox(h.payment_pool(), "PaymentCaptured").await;

    // All outboxes drained (negative assertion).
    h.assert_all_outboxes_drained().await;
}

// ── Scenario 2: Inventory Failure ──────────────────────────────────
//
// OrderCreated → InventoryReservationFailed → OrderCancelled
// Final: Order=Cancelled, stock unchanged, no payment

#[tokio::test]
async fn saga_inventory_failure() {
    let h = SagaHarness::start(MockPaymentGateway::always_succeeds()).await;

    // Seed: SKU with stock=1, order wants qty=5.
    let (_, sku_id, seller_id) = create_product_with_sku(h.catalog_pool(), 1).await;
    let items = vec![sample_order_item_with_sku(seller_id, sku_id, 5)];
    let (_order_id, _buyer_id) = create_order_via_service(h.order_pool(), items).await;

    // Step 1: Drain OrderCreated.
    let order_created = h.drain_outbox(h.order_pool(), "OrderCreated").await;
    let order_id = order_created.payload_uuid("order_id").unwrap();

    // Step 2: Deliver to catalog → fails (insufficient stock).
    // Handler returns Err(HandlerError::permanent), but InventoryReservationFailed
    // was written on a separate pool transaction (survives the error).
    let result = h.deliver_to_catalog(&order_created).await;
    assert!(result.is_err(), "expected inventory reservation to fail");

    // Step 3: Drain InventoryReservationFailed (written on pool, not rolled back).
    let inv_failed = h
        .drain_outbox(h.catalog_pool(), "InventoryReservationFailed")
        .await;

    // Step 4: Deliver to order → cancels order.
    h.deliver_to_order(&inv_failed).await;

    // ── Assertions ──────────────────────────────────────────────
    let order = order_repo::get_order_by_id(h.order_pool(), OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
    assert!(
        order.cancelled_reason.is_some(),
        "should have a cancellation reason"
    );

    // Stock unchanged.
    let stock: (i32,) = sqlx::query_as("SELECT stock_quantity FROM skus WHERE id = $1")
        .bind(sku_id)
        .fetch_one(h.catalog_pool())
        .await
        .unwrap();
    assert_eq!(stock.0, 1);

    // Reserved quantity is 0.
    let reserved: (i32,) = sqlx::query_as("SELECT reserved_quantity FROM skus WHERE id = $1")
        .bind(sku_id)
        .fetch_one(h.catalog_pool())
        .await
        .unwrap();
    assert_eq!(reserved.0, 0);

    // No payment records.
    let pay_txs = ledger_repo::list_transactions_by_order(h.payment_pool(), order_id)
        .await
        .unwrap();
    assert!(pay_txs.is_empty(), "no payment should have been created");

    // All outboxes drained.
    h.assert_all_outboxes_drained().await;
}

// ── Scenario 3: Payment Failure (gateway decline) ──────────────────
//
// OrderCreated → InventoryReserved → PaymentFailed → OrderCancelled → InventoryReleased
// Final: Order=Cancelled, inventory released, Payment=Failed

#[tokio::test]
async fn saga_payment_failure() {
    let h = SagaHarness::start(MockPaymentGateway::always_fails()).await;

    // Seed.
    let (_, sku_id, seller_id) = create_product_with_sku(h.catalog_pool(), 100).await;
    let items = vec![sample_order_item_with_sku(seller_id, sku_id, 2)];
    let (_order_id, _buyer_id) = create_order_via_service(h.order_pool(), items).await;

    // Steps 1-2: OrderCreated → catalog reserves.
    let order_created = h.drain_outbox(h.order_pool(), "OrderCreated").await;
    let order_id = order_created.payload_uuid("order_id").unwrap();
    h.deliver_to_catalog(&order_created).await.unwrap();

    // Step 3: Drain InventoryReserved.
    let inv_reserved = h.drain_outbox(h.catalog_pool(), "InventoryReserved").await;

    // Step 4: Deliver to order.
    h.deliver_to_order(&inv_reserved).await;

    // Step 5: Deliver to payment → gateway declines → PaymentFailed.
    h.deliver_to_payment(&inv_reserved).await;

    // Step 6: Drain PaymentFailed.
    let pay_failed = h.drain_outbox(h.payment_pool(), "PaymentFailed").await;

    // Step 7: Deliver to order → cancels order, writes OrderCancelled.
    h.deliver_to_order(&pay_failed).await;

    // Step 8: Drain OrderCancelled.
    let order_cancelled = h.drain_outbox(h.order_pool(), "OrderCancelled").await;

    // Step 9: Deliver to catalog → releases inventory.
    h.deliver_to_catalog(&order_cancelled).await.unwrap();

    // ── Assertions ──────────────────────────────────────────────
    let order = order_repo::get_order_by_id(h.order_pool(), OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);

    // Inventory released.
    let reserved: (i32,) = sqlx::query_as("SELECT reserved_quantity FROM skus WHERE id = $1")
        .bind(sku_id)
        .fetch_one(h.catalog_pool())
        .await
        .unwrap();
    assert_eq!(reserved.0, 0, "reserved should be released");

    // Payment state is Failed (or no transactions if decline prevented creation).
    let pay_txs = ledger_repo::list_transactions_by_order(h.payment_pool(), order_id)
        .await
        .unwrap();
    if !pay_txs.is_empty() {
        assert_eq!(
            ledger_repo::derive_payment_state(&pay_txs),
            PaymentState::Failed
        );
    }

    h.assert_all_outboxes_drained().await;
}

// ── Scenario 4: Payment Timeout ────────────────────────────────────
//
// Full flow through reservation, then PaymentTimedOut injected.
// OrderCreated → InventoryReserved → (timeout) → OrderCancelled → release + void
// Final: Order=Cancelled, inventory released

#[tokio::test]
async fn saga_payment_timeout() {
    let h = SagaHarness::start(MockPaymentGateway::always_succeeds()).await;

    // Seed.
    let (_, sku_id, seller_id) = create_product_with_sku(h.catalog_pool(), 100).await;
    let items = vec![sample_order_item_with_sku(seller_id, sku_id, 2)];
    let (_order_id, _buyer_id) = create_order_via_service(h.order_pool(), items).await;

    // Steps 1-4: OrderCreated → catalog reserves → order transitions to InventoryReserved.
    let order_created = h.drain_outbox(h.order_pool(), "OrderCreated").await;
    let order_id = order_created.payload_uuid("order_id").unwrap();
    h.deliver_to_catalog(&order_created).await.unwrap();
    let inv_reserved = h.drain_outbox(h.catalog_pool(), "InventoryReserved").await;
    h.deliver_to_order(&inv_reserved).await;
    // NOTE: We intentionally do NOT deliver InventoryReserved to payment (simulating timeout before auth).

    // Step 5: Inject PaymentTimedOut (synthetic — produced by external timeout monitor).
    let timeout_envelope =
        make_envelope(EventType::PaymentTimedOut, order_id, serde_json::json!({}));
    h.deliver_to_order(&timeout_envelope).await;

    // Step 6: Drain OrderCancelled.
    let order_cancelled = h.drain_outbox(h.order_pool(), "OrderCancelled").await;

    // Step 7: Deliver to catalog → release inventory.
    h.deliver_to_catalog(&order_cancelled).await.unwrap();

    // Step 8: Deliver to payment → void if authorized (no-op here since never authorized).
    h.deliver_to_payment(&order_cancelled).await;

    // ── Assertions ──────────────────────────────────────────────
    let order = order_repo::get_order_by_id(h.order_pool(), OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
    assert_eq!(order.cancelled_reason.as_deref(), Some("Payment timed out"));

    // Inventory released (was reserved, now 0).
    let reserved: (i32,) = sqlx::query_as("SELECT reserved_quantity FROM skus WHERE id = $1")
        .bind(sku_id)
        .fetch_one(h.catalog_pool())
        .await
        .unwrap();
    assert_eq!(reserved.0, 0, "reserved should be released after timeout");

    h.assert_all_outboxes_drained().await;
}

// ── Scenario 5a: Normal Cancel (after capture) ─────────────────────
//
// Full happy path → manual cancel → catalog releases, payment sees Captured (no-op).
// Final: Order=Cancelled, inventory released, Payment=Captured (not voided)

#[tokio::test]
async fn saga_cancel_after_capture() {
    let h = SagaHarness::start(MockPaymentGateway::always_succeeds()).await;

    // Seed.
    let (_, sku_id, seller_id) = create_product_with_sku(h.catalog_pool(), 100).await;
    let items = vec![sample_order_item_with_sku(seller_id, sku_id, 2)];
    let (_order_id, buyer_id) = create_order_via_service(h.order_pool(), items).await;

    // Full happy path (steps 1-9).
    let order_created = h.drain_outbox(h.order_pool(), "OrderCreated").await;
    let order_id = order_created.payload_uuid("order_id").unwrap();
    h.deliver_to_catalog(&order_created).await.unwrap();
    let inv_reserved = h.drain_outbox(h.catalog_pool(), "InventoryReserved").await;
    h.deliver_to_order(&inv_reserved).await;
    h.deliver_to_payment(&inv_reserved).await;
    let pay_authorized = h.drain_outbox(h.payment_pool(), "PaymentAuthorized").await;
    h.deliver_to_order(&pay_authorized).await;
    let order_confirmed = h.drain_outbox(h.order_pool(), "OrderConfirmed").await;
    h.deliver_to_payment(&order_confirmed).await;
    h.drain_outbox(h.payment_pool(), "PaymentCaptured").await;

    // Manual cancel via order service.
    let app_state = AppState::new_with_jwt(h.order_pool().clone(), test_auth_config());
    let current_user = CurrentUser {
        id: buyer_id,
        role: Role::Buyer,
    };
    order::orders::service::cancel_order(
        &app_state,
        &current_user,
        OrderId::new(order_id),
        Some("Changed my mind".to_string()),
    )
    .await
    .unwrap();

    // Drain OrderCancelled.
    let order_cancelled = h.drain_outbox(h.order_pool(), "OrderCancelled").await;

    // Deliver to catalog → release.
    h.deliver_to_catalog(&order_cancelled).await.unwrap();

    // Deliver to payment → sees Captured state, logs warning, no void.
    h.deliver_to_payment(&order_cancelled).await;

    // ── Assertions ──────────────────────────────────────────────
    let order = order_repo::get_order_by_id(h.order_pool(), OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);

    // Inventory released.
    let reserved: (i32,) = sqlx::query_as("SELECT reserved_quantity FROM skus WHERE id = $1")
        .bind(sku_id)
        .fetch_one(h.catalog_pool())
        .await
        .unwrap();
    assert_eq!(reserved.0, 0, "reserved should be released");

    // Payment stays Captured (void doesn't apply to captured payments).
    let pay_txs = ledger_repo::list_transactions_by_order(h.payment_pool(), order_id)
        .await
        .unwrap();
    assert_eq!(
        ledger_repo::derive_payment_state(&pay_txs),
        PaymentState::Captured
    );

    h.assert_all_outboxes_drained().await;
}

// ── Scenario 5b: Race Cancel (before capture) ──────────────────────
//
// Happy path through PaymentAuthorized → OrderConfirmed written but NOT delivered to payment.
// Cancel arrives first → payment voids the authorized payment.
// Final: Order=Cancelled, inventory released, Payment=Voided

#[tokio::test]
async fn saga_cancel_races_confirm() {
    let h = SagaHarness::start(MockPaymentGateway::always_succeeds()).await;

    // Seed.
    let (_, sku_id, seller_id) = create_product_with_sku(h.catalog_pool(), 100).await;
    let items = vec![sample_order_item_with_sku(seller_id, sku_id, 2)];
    let (_order_id, buyer_id) = create_order_via_service(h.order_pool(), items).await;

    // Steps 1-7: Up through auto-confirm (OrderConfirmed written to outbox but NOT delivered).
    let order_created = h.drain_outbox(h.order_pool(), "OrderCreated").await;
    let order_id = order_created.payload_uuid("order_id").unwrap();
    h.deliver_to_catalog(&order_created).await.unwrap();
    let inv_reserved = h.drain_outbox(h.catalog_pool(), "InventoryReserved").await;
    h.deliver_to_order(&inv_reserved).await;
    h.deliver_to_payment(&inv_reserved).await;
    let pay_authorized = h.drain_outbox(h.payment_pool(), "PaymentAuthorized").await;
    h.deliver_to_order(&pay_authorized).await;
    // OrderConfirmed is now in the order outbox — we drain it to "consume" it but DON'T deliver.
    let _order_confirmed = h.drain_outbox(h.order_pool(), "OrderConfirmed").await;

    // Manual cancel (races ahead of OrderConfirmed delivery to payment).
    let app_state = AppState::new_with_jwt(h.order_pool().clone(), test_auth_config());
    let current_user = CurrentUser {
        id: buyer_id,
        role: Role::Buyer,
    };
    order::orders::service::cancel_order(
        &app_state,
        &current_user,
        OrderId::new(order_id),
        Some("Cancel before capture".to_string()),
    )
    .await
    .unwrap();

    // Drain OrderCancelled.
    let order_cancelled = h.drain_outbox(h.order_pool(), "OrderCancelled").await;

    // Deliver to catalog → release.
    h.deliver_to_catalog(&order_cancelled).await.unwrap();

    // Deliver to payment → payment is Authorized, so it voids and writes PaymentVoided.
    h.deliver_to_payment(&order_cancelled).await;

    // Drain PaymentVoided from payment outbox.
    h.drain_outbox(h.payment_pool(), "PaymentVoided").await;

    // ── Assertions ──────────────────────────────────────────────
    let order = order_repo::get_order_by_id(h.order_pool(), OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);

    // Inventory released.
    let reserved: (i32,) = sqlx::query_as("SELECT reserved_quantity FROM skus WHERE id = $1")
        .bind(sku_id)
        .fetch_one(h.catalog_pool())
        .await
        .unwrap();
    assert_eq!(reserved.0, 0, "reserved should be released");

    // Payment voided (not captured).
    let pay_txs = ledger_repo::list_transactions_by_order(h.payment_pool(), order_id)
        .await
        .unwrap();
    assert_eq!(
        ledger_repo::derive_payment_state(&pay_txs),
        PaymentState::Voided
    );

    h.assert_all_outboxes_drained().await;
}
