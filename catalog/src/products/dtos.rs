use crate::products::entities::{ProductEntity, ProductImageEntity, SkuEntity};
use crate::products::repository;
use crate::products::value_objects::{
    Currency, ImageUrl, Price, ProductName, ProductStatus, SkuCode, SkuStatus, Slug, StockQuantity,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use shared::db::PgPool;
use shared::dto_helpers::{fmt_datetime, fmt_datetime_opt, fmt_id};
use shared::errors::AppError;
use uuid::Uuid;
// ── Product Request DTOs ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProductReq {
    pub name: String,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub base_price: Decimal,
    pub currency: Option<String>,
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProductReq {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub base_price: Option<Decimal>,
    pub currency: Option<String>,
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
    pub status: Option<ProductStatus>,
}

// ── SKU Request DTOs ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSkuReq {
    pub sku_code: String,
    pub price: Decimal,
    pub stock_quantity: i32,
    pub attributes: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSkuReq {
    pub price: Option<Decimal>,
    pub stock_quantity: Option<i32>,
    pub attributes: Option<serde_json::Value>,
    pub status: Option<SkuStatus>,
}

// ── Image Request DTOs ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddProductImageReq {
    pub url: String,
    pub alt_text: Option<String>,
    pub sort_order: Option<i32>,
    pub is_primary: Option<bool>,
}

// ── Response DTOs ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductRes {
    pub id: String,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub seller_id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub base_price: Decimal,
    pub currency: String,
    pub category_id: Option<String>,
    pub brand_id: Option<String>,
    pub status: ProductStatus,
    pub category_name: Option<String>,
    pub category_slug: Option<String>,
    pub brand_name: Option<String>,
    pub brand_slug: Option<String>,
}

impl ProductRes {
    pub fn new(entity: ProductEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            updated_at: fmt_datetime_opt(&entity.updated_at),
            seller_id: fmt_id(&entity.seller_id),
            name: entity.name,
            slug: entity.slug,
            description: entity.description,
            base_price: entity.base_price,
            currency: entity.currency,
            category_id: entity.category_id.map(|id| fmt_id(&id)),
            brand_id: entity.brand_id.map(|id| fmt_id(&id)),
            status: entity.status,
            category_name: entity.category_name,
            category_slug: entity.category_slug,
            brand_name: entity.brand_name,
            brand_slug: entity.brand_slug,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkuRes {
    pub id: String,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub product_id: String,
    pub sku_code: String,
    pub price: Decimal,
    pub stock_quantity: i32,
    pub attributes: serde_json::Value,
    pub status: SkuStatus,
}

impl SkuRes {
    pub fn new(entity: SkuEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            updated_at: fmt_datetime_opt(&entity.updated_at),
            product_id: fmt_id(&entity.product_id),
            sku_code: entity.sku_code,
            price: entity.price,
            stock_quantity: entity.stock_quantity,
            attributes: entity.attributes,
            status: entity.status,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductImageRes {
    pub id: String,
    pub created_at: String,
    pub product_id: String,
    pub url: String,
    pub alt_text: Option<String>,
    pub sort_order: i32,
    pub is_primary: bool,
}

impl ProductImageRes {
    pub fn new(entity: ProductImageEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            product_id: fmt_id(&entity.product_id),
            url: entity.url,
            alt_text: entity.alt_text,
            sort_order: entity.sort_order,
            is_primary: entity.is_primary,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductDetailRes {
    #[serde(flatten)]
    pub product: ProductRes,
    pub skus: Vec<SkuRes>,
    pub images: Vec<ProductImageRes>,
}

// ── Validated DTOs ──────────────────────────────────────────

/// Fully validated product for creation.
/// Enforces both value-object validation and FK referential integrity:
/// - All fields are validated value objects (name, slug, price, currency)
/// - category_id references an existing category (if set)
/// - brand_id references an existing brand (if set)
/// - brand is associated with category via brand_categories (if both set)
pub struct ValidCreateProductReq {
    pub name: ProductName,
    pub slug: Slug,
    pub description: Option<String>,
    pub base_price: Price,
    pub currency: Currency,
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
}

impl ValidCreateProductReq {
    pub async fn new(pool: &PgPool, req: CreateProductReq) -> Result<Self, AppError> {
        let name = ProductName::new(&req.name)?;
        let slug = match req.slug {
            Some(s) => Slug::new(&s)?,
            None => Slug::from_name(name.as_str())?,
        };

        validate_fk_references(pool, req.category_id, req.brand_id).await?;

        Ok(Self {
            name,
            slug,
            description: req.description,
            base_price: Price::new(req.base_price)?,
            currency: match req.currency {
                Some(c) => Currency::new(&c)?,
                None => Currency::default(),
            },
            category_id: req.category_id,
            brand_id: req.brand_id,
        })
    }
}

/// Fully validated product update.
/// Enforces value-object validation on provided fields, then validates
/// effective FK state (new values merged with existing product).
pub struct ValidUpdateProductReq {
    pub name: Option<ProductName>,
    pub slug: Option<Slug>,
    pub description: Option<String>,
    pub base_price: Option<Price>,
    pub currency: Option<Currency>,
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
    pub status: Option<ProductStatus>,
}

impl ValidUpdateProductReq {
    pub async fn new(
        pool: &PgPool,
        req: UpdateProductReq,
        existing: &ProductEntity,
    ) -> Result<Self, AppError> {
        // Merge: use updated value if present, else keep existing
        let effective_category = req.category_id.or(existing.category_id);
        let effective_brand = req.brand_id.or(existing.brand_id);
        validate_fk_references(pool, effective_category, effective_brand).await?;
        Ok(Self {
            name: req.name.map(|n| ProductName::new(&n)).transpose()?,
            slug: req.slug.map(|s| Slug::new(&s)).transpose()?,
            description: req.description,
            base_price: req.base_price.map(Price::new).transpose()?,
            currency: req.currency.map(|c| Currency::new(&c)).transpose()?,
            category_id: effective_category,
            brand_id: effective_brand,
            status: req.status,
        })
    }
}

/// Core FK validation: existence checks + brand-category association.
async fn validate_fk_references(
    pool: &PgPool,
    category_id: Option<Uuid>,
    brand_id: Option<Uuid>,
) -> Result<(), AppError> {
    if let Some(cat_id) = category_id {
        if !repository::category_exists(pool, cat_id).await? {
            return Err(AppError::BadRequest("Category does not exist".to_string()));
        }
    }
    if let Some(br_id) = brand_id {
        if !repository::brand_exists(pool, br_id).await? {
            return Err(AppError::BadRequest("Brand does not exist".to_string()));
        }
    }
    if let (Some(cat_id), Some(br_id)) = (category_id, brand_id) {
        if !repository::is_brand_in_category(pool, br_id, cat_id).await? {
            return Err(AppError::BadRequest(
                "Brand is not associated with the specified category".to_string(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ValidCreateSkuReq {
    pub sku_code: SkuCode,
    pub price: Price,
    pub stock_quantity: StockQuantity,
    pub attributes: serde_json::Value,
}

impl TryFrom<CreateSkuReq> for ValidCreateSkuReq {
    type Error = AppError;

    fn try_from(req: CreateSkuReq) -> Result<Self, Self::Error> {
        Ok(Self {
            sku_code: SkuCode::new(&req.sku_code)?,
            price: Price::new(req.price)?,
            stock_quantity: StockQuantity::new(req.stock_quantity)?,
            attributes: req.attributes.unwrap_or(serde_json::json!({})),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ValidUpdateSkuReq {
    pub price: Option<Price>,
    pub stock_quantity: Option<StockQuantity>,
    pub attributes: Option<serde_json::Value>,
    pub status: Option<SkuStatus>,
}

impl TryFrom<UpdateSkuReq> for ValidUpdateSkuReq {
    type Error = AppError;

    fn try_from(req: UpdateSkuReq) -> Result<Self, Self::Error> {
        let price = req.price.map(Price::new).transpose()?;
        let stock_quantity = req.stock_quantity.map(StockQuantity::new).transpose()?;

        Ok(Self {
            price,
            stock_quantity,
            attributes: req.attributes,
            status: req.status,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ValidAddProductImageReq {
    pub url: ImageUrl,
    pub alt_text: Option<String>,
    pub sort_order: i32,
    pub is_primary: bool,
}

impl TryFrom<AddProductImageReq> for ValidAddProductImageReq {
    type Error = AppError;

    fn try_from(req: AddProductImageReq) -> Result<Self, Self::Error> {
        Ok(Self {
            url: ImageUrl::new(&req.url)?,
            alt_text: req.alt_text,
            sort_order: req.sort_order.unwrap_or(0),
            is_primary: req.is_primary.unwrap_or(false),
        })
    }
}
