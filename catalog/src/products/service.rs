use crate::AppState;
use crate::products::dtos::{
    AddProductImageReq, CreateProductReq, CreateSkuReq, ProductDetailRes, ProductFilter,
    ProductImageRes, ProductListRes, ProductRes, SkuRes, UpdateProductReq, UpdateSkuReq,
    ValidAddProductImageReq, ValidCreateProductReq, ValidCreateSkuReq, ValidUpdateProductReq,
    ValidUpdateSkuReq,
};
use crate::products::repository;
use crate::products::value_objects::{ProductId, ProductImageId, SkuId};
use shared::auth::guards::require_access;
use shared::auth::jwt::CurrentUser;
use shared::db::pagination_support::{PaginationParams, PaginationRes, get_cursors};
use shared::db::transaction_support::{TxError, with_transaction};
use shared::errors::AppError;
use uuid::Uuid;

// ── Cache key helpers ─────────────────────────────────────

fn product_detail_key(id: ProductId) -> String {
    format!("product:{}", id.value())
}

fn product_slug_key(slug: &str) -> String {
    format!("product:slug:{slug}")
}

async fn evict_product_caches(state: &AppState, id: ProductId, slug: &str) {
    state.cache.evict(&product_detail_key(id)).await;
    state.cache.evict(&product_slug_key(slug)).await;
}

// ── Products ────────────────────────────────────────────

pub async fn create_product(
    state: &AppState,
    current_user: &CurrentUser,
    req: CreateProductReq,
) -> Result<ProductRes, AppError> {
    let validated = ValidCreateProductReq::new(req)?;
    repository::validate_fk_references(&state.pool, validated.category_id, validated.brand_id)
        .await?;
    let seller_id = current_user.id;

    let product_id = with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::create_product(tx.as_executor(), seller_id, validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create product: {}", e)))?;

    let product = repository::get_product_by_id(&state.pool, product_id).await?;
    Ok(ProductRes::new(product))
}

pub async fn get_product(state: &AppState, id: ProductId) -> Result<ProductRes, AppError> {
    let product = repository::get_product_by_id(&state.pool, id).await?;
    Ok(ProductRes::new(product))
}

pub async fn get_product_by_slug(state: &AppState, slug: &str) -> Result<ProductRes, AppError> {
    let cache_key = product_slug_key(slug);
    if let Some(cached) = state.cache.get::<ProductRes>(&cache_key).await {
        return Ok(cached);
    }

    let product = repository::get_product_by_slug(&state.pool, slug).await?;
    let res = ProductRes::new(product);
    state.cache.set(&cache_key, &res).await;
    Ok(res)
}

pub async fn get_product_detail(
    state: &AppState,
    id: ProductId,
) -> Result<ProductDetailRes, AppError> {
    let cache_key = product_detail_key(id);
    if let Some(cached) = state.cache.get::<ProductDetailRes>(&cache_key).await {
        return Ok(cached);
    }

    let row = repository::get_product_detail(&state.pool, id).await?;
    let detail = ProductDetailRes::from_row(row)?;

    state.cache.set(&cache_key, &detail).await;
    Ok(detail)
}

pub async fn list_products_by_seller(
    state: &AppState,
    seller_id: Uuid,
    params: PaginationParams,
    filter: ProductFilter,
) -> Result<PaginationRes<ProductListRes>, AppError> {
    let mut products =
        repository::list_products_by_seller(&state.pool, seller_id, &params, &filter).await?;
    let cursors = get_cursors(&params, &mut products);
    let items = products.into_iter().map(ProductListRes::new).collect();
    Ok(PaginationRes::new(items, cursors))
}

pub async fn list_active_products(
    state: &AppState,
    params: PaginationParams,
    filter: ProductFilter,
) -> Result<PaginationRes<ProductListRes>, AppError> {
    let mut products = repository::list_active_products(&state.pool, &params, &filter).await?;
    let cursors = get_cursors(&params, &mut products);
    let items = products.into_iter().map(ProductListRes::new).collect();
    Ok(PaginationRes::new(items, cursors))
}

pub async fn update_product(
    state: &AppState,
    current_user: &CurrentUser,
    product_id: ProductId,
    req: UpdateProductReq,
) -> Result<(), AppError> {
    let product = repository::get_product_by_id(&state.pool, product_id).await?;
    require_access(current_user, &product.seller_id)?;

    let validated = ValidUpdateProductReq::new(req, &product)?;
    repository::validate_fk_references(&state.pool, validated.category_id, validated.brand_id)
        .await?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::update_product(tx.as_executor(), product_id, validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to update product: {}", e)))?;

    evict_product_caches(state, product_id, &product.slug).await;
    Ok(())
}

pub async fn delete_product(
    state: &AppState,
    current_user: &CurrentUser,
    product_id: ProductId,
) -> Result<(), AppError> {
    let product = repository::get_product_by_id(&state.pool, product_id).await?;
    require_access(current_user, &product.seller_id)?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::delete_product(tx.as_executor(), product_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to delete product: {}", e)))?;

    evict_product_caches(state, product_id, &product.slug).await;
    Ok(())
}

// ── SKUs ────────────────────────────────────────────────

pub async fn create_sku(
    state: &AppState,
    current_user: &CurrentUser,
    product_id: ProductId,
    req: CreateSkuReq,
) -> Result<SkuRes, AppError> {
    let product = repository::get_product_by_id(&state.pool, product_id).await?;
    require_access(current_user, &product.seller_id)?;

    let validated: ValidCreateSkuReq = req.try_into()?;

    let sku_id = with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::create_sku(tx.as_executor(), product_id, validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create SKU: {}", e)))?;

    state.cache.evict(&product_detail_key(product_id)).await;

    let sku = repository::get_sku_by_id(&state.pool, sku_id).await?;
    Ok(SkuRes::new(sku))
}

pub async fn list_skus(state: &AppState, product_id: ProductId) -> Result<Vec<SkuRes>, AppError> {
    let skus = repository::list_skus_by_product(&state.pool, product_id).await?;
    Ok(skus.into_iter().map(SkuRes::new).collect())
}

pub async fn update_sku(
    state: &AppState,
    current_user: &CurrentUser,
    sku_id: SkuId,
    req: UpdateSkuReq,
) -> Result<(), AppError> {
    let sku = repository::get_sku_by_id(&state.pool, sku_id).await?;
    let product =
        repository::get_product_by_id(&state.pool, ProductId::new(sku.product_id)).await?;
    require_access(current_user, &product.seller_id)?;

    let validated: ValidUpdateSkuReq = req.try_into()?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::update_sku(tx.as_executor(), sku_id, validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to update SKU: {}", e)))?;

    state
        .cache
        .evict(&product_detail_key(ProductId::new(sku.product_id)))
        .await;
    Ok(())
}

pub async fn delete_sku(
    state: &AppState,
    current_user: &CurrentUser,
    sku_id: SkuId,
) -> Result<(), AppError> {
    let sku = repository::get_sku_by_id(&state.pool, sku_id).await?;
    let product =
        repository::get_product_by_id(&state.pool, ProductId::new(sku.product_id)).await?;
    require_access(current_user, &product.seller_id)?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::delete_sku(tx.as_executor(), sku_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to delete SKU: {}", e)))?;

    state
        .cache
        .evict(&product_detail_key(ProductId::new(sku.product_id)))
        .await;
    Ok(())
}

pub async fn adjust_stock(
    state: &AppState,
    current_user: &CurrentUser,
    sku_id: SkuId,
    delta: i32,
) -> Result<(), AppError> {
    let sku = repository::get_sku_by_id(&state.pool, sku_id).await?;
    let product =
        repository::get_product_by_id(&state.pool, ProductId::new(sku.product_id)).await?;
    require_access(current_user, &product.seller_id)?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::adjust_stock(tx.as_executor(), sku_id, delta)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to adjust stock: {}", e)))?;

    state
        .cache
        .evict(&product_detail_key(ProductId::new(sku.product_id)))
        .await;
    Ok(())
}

// ── Images ──────────────────────────────────────────────

pub async fn list_images(
    state: &AppState,
    product_id: ProductId,
) -> Result<Vec<ProductImageRes>, AppError> {
    let images = repository::list_images_by_product(&state.pool, product_id).await?;
    Ok(images.into_iter().map(ProductImageRes::new).collect())
}

pub async fn add_image(
    state: &AppState,
    current_user: &CurrentUser,
    product_id: ProductId,
    req: AddProductImageReq,
) -> Result<ProductImageRes, AppError> {
    let product = repository::get_product_by_id(&state.pool, product_id).await?;
    require_access(current_user, &product.seller_id)?;

    let validated: ValidAddProductImageReq = req.try_into()?;

    let image_id = with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::add_product_image(tx.as_executor(), product_id, validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to add image: {}", e)))?;

    state.cache.evict(&product_detail_key(product_id)).await;

    let image = repository::get_image_by_id(&state.pool, image_id).await?;
    Ok(ProductImageRes::new(image))
}

pub async fn delete_image(
    state: &AppState,
    current_user: &CurrentUser,
    product_id: ProductId,
    image_id: ProductImageId,
) -> Result<(), AppError> {
    let product = repository::get_product_by_id(&state.pool, product_id).await?;
    require_access(current_user, &product.seller_id)?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::delete_product_image(tx.as_executor(), image_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to delete image: {}", e)))?;

    state.cache.evict(&product_detail_key(product_id)).await;
    Ok(())
}
