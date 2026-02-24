use crate::products::dtos::{
    ValidAddProductImageReq, ValidCreateProductReq, ValidCreateSkuReq, ValidUpdateProductReq,
    ValidUpdateSkuReq,
};
use crate::products::entities::{ProductEntity, ProductImageEntity, SkuEntity};
use shared::db::PgExec;
use shared::errors::AppError;
use sqlx::PgConnection;
use uuid::Uuid;

// ── Product queries ─────────────────────────────────────────

pub async fn get_product_by_id<'e>(
    executor: impl PgExec<'e>,
    id: Uuid,
) -> Result<ProductEntity, AppError> {
    sqlx::query_as::<_, ProductEntity>(
        "SELECT * FROM products WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::NotFound(format!("Product not found: {}", e)))
}

pub async fn get_product_by_slug<'e>(
    executor: impl PgExec<'e>,
    slug: &str,
) -> Result<ProductEntity, AppError> {
    sqlx::query_as::<_, ProductEntity>(
        "SELECT * FROM products WHERE slug = $1 AND deleted_at IS NULL",
    )
    .bind(slug)
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::NotFound(format!("Product not found: {}", e)))
}

pub async fn list_products_by_seller<'e>(
    executor: impl PgExec<'e>,
    seller_id: Uuid,
) -> Result<Vec<ProductEntity>, AppError> {
    sqlx::query_as::<_, ProductEntity>(
        "SELECT * FROM products WHERE seller_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
    )
    .bind(seller_id)
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to list products: {}", e)))
}

pub async fn list_active_products<'e>(
    executor: impl PgExec<'e>,
) -> Result<Vec<ProductEntity>, AppError> {
    sqlx::query_as::<_, ProductEntity>(
        "SELECT * FROM products WHERE status = 'active' AND deleted_at IS NULL ORDER BY created_at DESC",
    )
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to list products: {}", e)))
}

pub async fn create_product(
    tx: &mut PgConnection,
    seller_id: Uuid,
    req: ValidCreateProductReq,
) -> Result<Uuid, AppError> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO products (seller_id, name, slug, description, base_price, currency, category, brand)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
    )
    .bind(seller_id)
    .bind(req.name.as_str())
    .bind(req.slug.as_str())
    .bind(&req.description)
    .bind(req.base_price.value())
    .bind(req.currency.as_str())
    .bind(&req.category)
    .bind(&req.brand)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create product: {}", e)))?;

    Ok(row.0)
}

pub async fn update_product(
    tx: &mut PgConnection,
    id: Uuid,
    req: ValidUpdateProductReq,
) -> Result<(), AppError> {
    // Build dynamic SET clause for partial updates
    let mut set_parts: Vec<String> = Vec::new();
    let mut param_idx = 2u32; // $1 is the id

    if req.name.is_some() {
        set_parts.push(format!("name = ${}", param_idx));
        param_idx += 1;
    }
    if req.slug.is_some() {
        set_parts.push(format!("slug = ${}", param_idx));
        param_idx += 1;
    }
    if req.description.is_some() {
        set_parts.push(format!("description = ${}", param_idx));
        param_idx += 1;
    }
    if req.base_price.is_some() {
        set_parts.push(format!("base_price = ${}", param_idx));
        param_idx += 1;
    }
    if req.currency.is_some() {
        set_parts.push(format!("currency = ${}", param_idx));
        param_idx += 1;
    }
    if req.category.is_some() {
        set_parts.push(format!("category = ${}", param_idx));
        param_idx += 1;
    }
    if req.brand.is_some() {
        set_parts.push(format!("brand = ${}", param_idx));
        param_idx += 1;
    }
    if req.status.is_some() {
        set_parts.push(format!("status = ${}", param_idx));
        param_idx += 1;
    }

    if set_parts.is_empty() {
        return Ok(());
    }

    set_parts.push("updated_at = NOW()".to_string());
    let _ = param_idx;

    let sql = format!(
        "UPDATE products SET {} WHERE id = $1 AND deleted_at IS NULL",
        set_parts.join(", ")
    );

    let mut query = sqlx::query(&sql).bind(id);

    if let Some(ref name) = req.name {
        query = query.bind(name.as_str());
    }
    if let Some(ref slug) = req.slug {
        query = query.bind(slug.as_str());
    }
    if let Some(ref description) = req.description {
        query = query.bind(description);
    }
    if let Some(ref base_price) = req.base_price {
        query = query.bind(base_price.value());
    }
    if let Some(ref currency) = req.currency {
        query = query.bind(currency.as_str());
    }
    if let Some(ref category) = req.category {
        query = query.bind(category);
    }
    if let Some(ref brand) = req.brand {
        query = query.bind(brand);
    }
    if let Some(ref status) = req.status {
        query = query.bind(status.as_str());
    }

    let result = query
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update product: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Product not found".to_string()));
    }

    Ok(())
}

pub async fn delete_product(tx: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
    let result =
        sqlx::query("UPDATE products SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                AppError::InternalServerError(format!("Failed to delete product: {}", e))
            })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Product not found".to_string()));
    }

    Ok(())
}

// ── SKU queries ─────────────────────────────────────────────

pub async fn get_sku_by_id<'e>(executor: impl PgExec<'e>, id: Uuid) -> Result<SkuEntity, AppError> {
    sqlx::query_as::<_, SkuEntity>("SELECT * FROM skus WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("SKU not found: {}", e)))
}

pub async fn list_skus_by_product<'e>(
    executor: impl PgExec<'e>,
    product_id: Uuid,
) -> Result<Vec<SkuEntity>, AppError> {
    sqlx::query_as::<_, SkuEntity>(
        "SELECT * FROM skus WHERE product_id = $1 AND deleted_at IS NULL ORDER BY created_at ASC",
    )
    .bind(product_id)
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to list SKUs: {}", e)))
}

pub async fn create_sku(
    tx: &mut PgConnection,
    product_id: Uuid,
    req: ValidCreateSkuReq,
) -> Result<Uuid, AppError> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO skus (product_id, sku_code, price, stock_quantity, attributes)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
    )
    .bind(product_id)
    .bind(req.sku_code.as_str())
    .bind(req.price.value())
    .bind(req.stock_quantity.value())
    .bind(&req.attributes)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create SKU: {}", e)))?;

    Ok(row.0)
}

pub async fn update_sku(
    tx: &mut PgConnection,
    id: Uuid,
    req: ValidUpdateSkuReq,
) -> Result<(), AppError> {
    let mut set_parts: Vec<String> = Vec::new();
    let mut param_idx = 2u32;

    if req.price.is_some() {
        set_parts.push(format!("price = ${}", param_idx));
        param_idx += 1;
    }
    if req.stock_quantity.is_some() {
        set_parts.push(format!("stock_quantity = ${}", param_idx));
        param_idx += 1;
    }
    if req.attributes.is_some() {
        set_parts.push(format!("attributes = ${}", param_idx));
        param_idx += 1;
    }
    if req.status.is_some() {
        set_parts.push(format!("status = ${}", param_idx));
        param_idx += 1;
    }

    if set_parts.is_empty() {
        return Ok(());
    }

    set_parts.push("updated_at = NOW()".to_string());
    let _ = param_idx;

    let sql = format!(
        "UPDATE skus SET {} WHERE id = $1 AND deleted_at IS NULL",
        set_parts.join(", ")
    );

    let mut query = sqlx::query(&sql).bind(id);

    if let Some(ref price) = req.price {
        query = query.bind(price.value());
    }
    if let Some(ref stock_quantity) = req.stock_quantity {
        query = query.bind(stock_quantity.value());
    }
    if let Some(ref attributes) = req.attributes {
        query = query.bind(attributes);
    }
    if let Some(ref status) = req.status {
        query = query.bind(status.as_str());
    }

    let result = query
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update SKU: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("SKU not found".to_string()));
    }

    Ok(())
}

pub async fn delete_sku(tx: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
    let result =
        sqlx::query("UPDATE skus SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::InternalServerError(format!("Failed to delete SKU: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("SKU not found".to_string()));
    }

    Ok(())
}

// ── Stock operations ────────────────────────────────────────

pub async fn adjust_stock(tx: &mut PgConnection, sku_id: Uuid, delta: i32) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE skus SET stock_quantity = stock_quantity + $1, updated_at = NOW()
         WHERE id = $2 AND deleted_at IS NULL AND stock_quantity + $1 >= 0",
    )
    .bind(delta)
    .bind(sku_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to adjust stock: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::BadRequest(
            "SKU not found or insufficient stock".to_string(),
        ));
    }

    Ok(())
}

// ── Image queries ───────────────────────────────────────────

pub async fn list_images_by_product<'e>(
    executor: impl PgExec<'e>,
    product_id: Uuid,
) -> Result<Vec<ProductImageEntity>, AppError> {
    sqlx::query_as::<_, ProductImageEntity>(
        "SELECT * FROM product_images WHERE product_id = $1 ORDER BY sort_order ASC",
    )
    .bind(product_id)
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to list images: {}", e)))
}

pub async fn add_product_image(
    tx: &mut PgConnection,
    product_id: Uuid,
    req: ValidAddProductImageReq,
) -> Result<Uuid, AppError> {
    // If this image is primary, unset any existing primary
    if req.is_primary {
        sqlx::query("UPDATE product_images SET is_primary = FALSE WHERE product_id = $1 AND is_primary = TRUE")
            .bind(product_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::InternalServerError(format!("Failed to unset primary image: {}", e)))?;
    }

    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO product_images (product_id, url, alt_text, sort_order, is_primary)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
    )
    .bind(product_id)
    .bind(req.url.as_str())
    .bind(&req.alt_text)
    .bind(req.sort_order)
    .bind(req.is_primary)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to add image: {}", e)))?;

    Ok(row.0)
}

pub async fn delete_product_image(tx: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM product_images WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete image: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Image not found".to_string()));
    }

    Ok(())
}
