use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use shared::errors::AppError;
use uuid::Uuid;

const CART_TTL_SECS: i64 = 2_592_000; // 30 days

/// Intermediate type for Redis JSON storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartItemStored {
    pub product_id: Uuid,
    pub sku_id: Uuid,
    pub quantity: u32,
    pub unit_price: String, // Decimal as string for precise JSON roundtrip
    pub currency: String,
    pub product_name: String,
    pub image_url: Option<String>,
    pub added_at: DateTime<Utc>,
}

fn cart_key(user_id: Uuid) -> String {
    format!("cart:{user_id}")
}

pub async fn get_cart(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
) -> Result<Vec<CartItemStored>, AppError> {
    let items: std::collections::HashMap<String, String> = conn
        .hgetall(cart_key(user_id))
        .await
        .map_err(|e| AppError::InternalServerError(format!("Redis HGETALL failed: {e}")))?;

    let mut result = Vec::with_capacity(items.len());
    for (_sku_id, json_str) in items {
        let stored: CartItemStored = serde_json::from_str(&json_str).map_err(|e| {
            AppError::InternalServerError(format!("Cart item deserialization: {e}"))
        })?;
        result.push(stored);
    }
    Ok(result)
}

pub async fn get_cart_item(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
    sku_id: Uuid,
) -> Result<Option<CartItemStored>, AppError> {
    let json_str: Option<String> = conn
        .hget(cart_key(user_id), sku_id.to_string())
        .await
        .map_err(|e| AppError::InternalServerError(format!("Redis HGET failed: {e}")))?;

    match json_str {
        Some(s) => {
            let stored: CartItemStored = serde_json::from_str(&s).map_err(|e| {
                AppError::InternalServerError(format!("Cart item deserialization: {e}"))
            })?;
            Ok(Some(stored))
        }
        None => Ok(None),
    }
}

pub async fn cart_item_count(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
) -> Result<u64, AppError> {
    let count: u64 = conn
        .hlen(cart_key(user_id))
        .await
        .map_err(|e| AppError::InternalServerError(format!("Redis HLEN failed: {e}")))?;
    Ok(count)
}

pub async fn item_exists(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
    sku_id: Uuid,
) -> Result<bool, AppError> {
    let exists: bool = conn
        .hexists(cart_key(user_id), sku_id.to_string())
        .await
        .map_err(|e| AppError::InternalServerError(format!("Redis HEXISTS failed: {e}")))?;
    Ok(exists)
}

pub async fn set_cart_item(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
    sku_id: Uuid,
    stored: &CartItemStored,
) -> Result<(), AppError> {
    let key = cart_key(user_id);
    let json_str = serde_json::to_string(stored)
        .map_err(|e| AppError::InternalServerError(format!("Cart item serialization: {e}")))?;

    let _: () = conn
        .hset(&key, sku_id.to_string(), json_str)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Redis HSET failed: {e}")))?;

    // Refresh TTL on write
    let _: () = conn
        .expire(&key, CART_TTL_SECS)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Redis EXPIRE failed: {e}")))?;

    Ok(())
}

pub async fn remove_cart_item(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
    sku_id: Uuid,
) -> Result<(), AppError> {
    let key = cart_key(user_id);

    let _: () = conn
        .hdel(&key, sku_id.to_string())
        .await
        .map_err(|e| AppError::InternalServerError(format!("Redis HDEL failed: {e}")))?;

    // Refresh TTL on write
    let _: () = conn
        .expire(&key, CART_TTL_SECS)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Redis EXPIRE failed: {e}")))?;

    Ok(())
}

pub async fn clear_cart(
    conn: &mut redis::aio::ConnectionManager,
    user_id: Uuid,
) -> Result<(), AppError> {
    let _: () = conn
        .del(cart_key(user_id))
        .await
        .map_err(|e| AppError::InternalServerError(format!("Redis DEL failed: {e}")))?;
    Ok(())
}
