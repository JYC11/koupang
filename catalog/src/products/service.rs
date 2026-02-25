use crate::products::dtos::{
    AddProductImageReq, CreateProductReq, CreateSkuReq, ProductDetailRes, ProductImageRes,
    ProductRes, SkuRes, UpdateProductReq, UpdateSkuReq, ValidAddProductImageReq,
    ValidCreateProductReq, ValidCreateSkuReq, ValidUpdateProductReq, ValidUpdateSkuReq,
};
use crate::products::repository;
use crate::products::value_objects::{ProductId, ProductImageId, SkuId};
use shared::auth::guards::require_access;
use shared::auth::jwt::CurrentUser;
use shared::db::PgPool;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::errors::AppError;
use uuid::Uuid;

pub struct CatalogService {
    pool: PgPool,
}

impl CatalogService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // ── Products ────────────────────────────────────────────

    pub async fn create_product(
        &self,
        current_user: &CurrentUser,
        req: CreateProductReq,
    ) -> Result<ProductRes, AppError> {
        let validated = ValidCreateProductReq::new(&self.pool, req).await?;
        let seller_id = current_user.id;

        let product_id = with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::create_product(tx.as_executor(), seller_id, validated)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to create product: {}", e)))?;

        let product = repository::get_product_by_id(&self.pool, product_id).await?;
        Ok(ProductRes::new(product))
    }

    pub async fn get_product(&self, id: ProductId) -> Result<ProductRes, AppError> {
        let product = repository::get_product_by_id(&self.pool, id).await?;
        Ok(ProductRes::new(product))
    }

    pub async fn get_product_by_slug(&self, slug: &str) -> Result<ProductRes, AppError> {
        let product = repository::get_product_by_slug(&self.pool, slug).await?;
        Ok(ProductRes::new(product))
    }

    pub async fn get_product_detail(&self, id: ProductId) -> Result<ProductDetailRes, AppError> {
        let product = repository::get_product_by_id(&self.pool, id).await?;
        let skus = repository::list_skus_by_product(&self.pool, id).await?;
        let images = repository::list_images_by_product(&self.pool, id).await?;

        Ok(ProductDetailRes {
            product: ProductRes::new(product),
            skus: skus.into_iter().map(SkuRes::new).collect(),
            images: images.into_iter().map(ProductImageRes::new).collect(),
        })
    }

    pub async fn list_products_by_seller(
        &self,
        seller_id: Uuid,
    ) -> Result<Vec<ProductRes>, AppError> {
        let products = repository::list_products_by_seller(&self.pool, seller_id).await?;
        Ok(products.into_iter().map(ProductRes::new).collect())
    }

    pub async fn list_active_products(&self) -> Result<Vec<ProductRes>, AppError> {
        let products = repository::list_active_products(&self.pool).await?;
        Ok(products.into_iter().map(ProductRes::new).collect())
    }

    pub async fn update_product(
        &self,
        current_user: &CurrentUser,
        product_id: ProductId,
        req: UpdateProductReq,
    ) -> Result<(), AppError> {
        let product = repository::get_product_by_id(&self.pool, product_id).await?;
        require_access(current_user, &product.seller_id)?;

        let validated = ValidUpdateProductReq::new(&self.pool, req, &product).await?;

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::update_product(tx.as_executor(), product_id, validated)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update product: {}", e)))?;

        Ok(())
    }

    pub async fn delete_product(
        &self,
        current_user: &CurrentUser,
        product_id: ProductId,
    ) -> Result<(), AppError> {
        let product = repository::get_product_by_id(&self.pool, product_id).await?;
        require_access(current_user, &product.seller_id)?;

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::delete_product(tx.as_executor(), product_id)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete product: {}", e)))?;

        Ok(())
    }

    // ── SKUs ────────────────────────────────────────────────

    pub async fn create_sku(
        &self,
        current_user: &CurrentUser,
        product_id: ProductId,
        req: CreateSkuReq,
    ) -> Result<SkuRes, AppError> {
        let product = repository::get_product_by_id(&self.pool, product_id).await?;
        require_access(current_user, &product.seller_id)?;

        let validated: ValidCreateSkuReq = req.try_into()?;

        let sku_id = with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::create_sku(tx.as_executor(), product_id, validated)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to create SKU: {}", e)))?;

        let sku = repository::get_sku_by_id(&self.pool, sku_id).await?;
        Ok(SkuRes::new(sku))
    }

    pub async fn list_skus(&self, product_id: ProductId) -> Result<Vec<SkuRes>, AppError> {
        let skus = repository::list_skus_by_product(&self.pool, product_id).await?;
        Ok(skus.into_iter().map(SkuRes::new).collect())
    }

    pub async fn update_sku(
        &self,
        current_user: &CurrentUser,
        sku_id: SkuId,
        req: UpdateSkuReq,
    ) -> Result<(), AppError> {
        let sku = repository::get_sku_by_id(&self.pool, sku_id).await?;
        let product =
            repository::get_product_by_id(&self.pool, ProductId::new(sku.product_id)).await?;
        require_access(current_user, &product.seller_id)?;

        let validated: ValidUpdateSkuReq = req.try_into()?;

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::update_sku(tx.as_executor(), sku_id, validated)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update SKU: {}", e)))?;

        Ok(())
    }

    pub async fn delete_sku(
        &self,
        current_user: &CurrentUser,
        sku_id: SkuId,
    ) -> Result<(), AppError> {
        let sku = repository::get_sku_by_id(&self.pool, sku_id).await?;
        let product =
            repository::get_product_by_id(&self.pool, ProductId::new(sku.product_id)).await?;
        require_access(current_user, &product.seller_id)?;

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::delete_sku(tx.as_executor(), sku_id)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete SKU: {}", e)))?;

        Ok(())
    }

    pub async fn adjust_stock(
        &self,
        current_user: &CurrentUser,
        sku_id: SkuId,
        delta: i32,
    ) -> Result<(), AppError> {
        let sku = repository::get_sku_by_id(&self.pool, sku_id).await?;
        let product =
            repository::get_product_by_id(&self.pool, ProductId::new(sku.product_id)).await?;
        require_access(current_user, &product.seller_id)?;

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::adjust_stock(tx.as_executor(), sku_id, delta)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to adjust stock: {}", e)))?;

        Ok(())
    }

    // ── Images ──────────────────────────────────────────────

    pub async fn list_images(
        &self,
        product_id: ProductId,
    ) -> Result<Vec<ProductImageRes>, AppError> {
        let images = repository::list_images_by_product(&self.pool, product_id).await?;
        Ok(images.into_iter().map(ProductImageRes::new).collect())
    }

    pub async fn add_image(
        &self,
        current_user: &CurrentUser,
        product_id: ProductId,
        req: AddProductImageReq,
    ) -> Result<ProductImageRes, AppError> {
        let product = repository::get_product_by_id(&self.pool, product_id).await?;
        require_access(current_user, &product.seller_id)?;

        let validated: ValidAddProductImageReq = req.try_into()?;

        let image_id = with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::add_product_image(tx.as_executor(), product_id, validated)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to add image: {}", e)))?;

        // Fetch the newly created image for response
        let images = repository::list_images_by_product(&self.pool, product_id).await?;
        let image = images
            .into_iter()
            .find(|img| img.id == image_id.value())
            .ok_or_else(|| {
                AppError::InternalServerError("Image not found after insert".to_string())
            })?;

        Ok(ProductImageRes::new(image))
    }

    pub async fn delete_image(
        &self,
        current_user: &CurrentUser,
        product_id: ProductId,
        image_id: ProductImageId,
    ) -> Result<(), AppError> {
        let product = repository::get_product_by_id(&self.pool, product_id).await?;
        require_access(current_user, &product.seller_id)?;

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::delete_product_image(tx.as_executor(), image_id)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete image: {}", e)))?;

        Ok(())
    }
}
