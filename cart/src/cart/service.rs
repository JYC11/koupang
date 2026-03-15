use crate::cart::domain::Cart;
use crate::cart::dtos::{
    AddToCartReq, CartRes, CartValidationItemRes, CartValidationRes, UpdateCartItemReq,
    ValidAddToCartReq, ValidUpdateCartItemReq,
};
use crate::cart::repository;
use shared::errors::AppError;
use uuid::Uuid;

const MAX_CART_ITEMS: u64 = 50;

pub async fn get_cart(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
) -> Result<CartRes, AppError> {
    let stored_items = repository::get_cart(conn, user_id).await?;
    let domain_items: Vec<_> = stored_items
        .into_iter()
        .map(|s| s.to_domain())
        .collect::<Result<_, _>>()?;
    let cart = Cart::new(user_id, domain_items);
    Ok(CartRes::from_cart(&cart))
}

pub async fn add_item(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
    req: AddToCartReq,
) -> Result<CartRes, AppError> {
    let validated: ValidAddToCartReq = req.try_into()?;

    // Check max items (only if adding a new SKU)
    let exists = repository::item_exists(conn, user_id, validated.sku_id).await?;
    if !exists {
        let count = repository::cart_item_count(conn, user_id).await?;
        if count >= MAX_CART_ITEMS {
            return Err(AppError::BadRequest(format!(
                "Cart is full (max {MAX_CART_ITEMS} items)"
            )));
        }
    }

    let stored = validated.to_stored();
    repository::set_cart_item(conn, user_id, validated.sku_id, &stored).await?;

    get_cart(conn, user_id).await
}

pub async fn update_item_quantity(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
    sku_id: Uuid,
    req: UpdateCartItemReq,
) -> Result<CartRes, AppError> {
    let validated: ValidUpdateCartItemReq = req.try_into()?;

    let existing = repository::get_cart_item(conn, user_id, sku_id)
        .await?
        .ok_or(AppError::NotFound("Cart item not found".to_string()))?;

    let mut updated = existing;
    updated.quantity = validated.quantity.value();

    repository::set_cart_item(conn, user_id, sku_id, &updated).await?;

    get_cart(conn, user_id).await
}

pub async fn remove_item(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
    sku_id: Uuid,
) -> Result<(), AppError> {
    repository::remove_cart_item(conn, user_id, sku_id).await
}

pub async fn clear_cart(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
) -> Result<(), AppError> {
    repository::clear_cart(conn, user_id).await
}

/// Stub: returns all items as valid until catalog integration is built.
pub async fn validate_cart(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
) -> Result<CartValidationRes, AppError> {
    let stored_items = repository::get_cart(conn, user_id).await?;

    let items: Vec<CartValidationItemRes> = stored_items
        .iter()
        .map(|item| {
            let price: rust_decimal::Decimal = item.unit_price.parse().unwrap_or_default();
            CartValidationItemRes {
                sku_id: item.sku_id.to_string(),
                price_changed: false,
                snapshot_price: price,
                current_price: Some(price), // Stub: assume same price
                product_unavailable: false,
                stock_insufficient: false,
            }
        })
        .collect();

    Ok(CartValidationRes {
        all_valid: true,
        items,
    })
}
