use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use uuid::Uuid;

use crate::AppState;
use crate::ledger::repository as ledger_repo;
use crate::payments::dtos::{AccountBalanceRes, PaymentStatusRes, TransactionRes};
use shared::auth::jwt::CurrentUser;
use shared::auth::middleware::AuthMiddleware;
use shared::dto_helpers::fmt_id;
use shared::errors::AppError;

pub fn payment_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new_claims_based(app_state.auth_config.clone());

    let protected_routes = Router::new()
        .route("/{order_id}", get(get_payment_status))
        .layer(axum::middleware::from_fn(move |req, next| {
            auth_middleware.clone().handle(req, next)
        }));

    Router::new()
        .nest("/api/v1/payments", protected_routes)
        .with_state(app_state)
}

async fn get_payment_status(
    State(state): State<AppState>,
    Path(order_id): Path<Uuid>,
    _current_user: CurrentUser,
) -> Result<Json<PaymentStatusRes>, AppError> {
    let transactions = ledger_repo::list_transactions_by_order(&state.pool, order_id).await?;
    let payment_state = ledger_repo::derive_payment_state(&transactions);

    let mut tx_responses = Vec::with_capacity(transactions.len());
    for tx in &transactions {
        let entries = ledger_repo::list_entries_by_transaction(&state.pool, tx.id).await?;
        tx_responses.push(TransactionRes::new(tx.clone(), entries));
    }

    let balances = ledger_repo::get_account_balances(&state.pool, order_id).await?;

    Ok(Json(PaymentStatusRes {
        order_id: fmt_id(&order_id),
        state: payment_state,
        transactions: tx_responses,
        balances: balances.into_iter().map(AccountBalanceRes::new).collect(),
    }))
}
