use crate::brands::value_objects::BrandId;
use crate::categories::value_objects::CategoryId;
use crate::products::entities::{
    ProductDetailRow, ProductEntity, ProductImageEntity, ProductListEntity, SkuEntity,
};
use crate::products::value_objects::{
    Currency, ImageUrl, Price, ProductName, ProductStatus, SearchQuery, SkuCode, SkuStatus, Slug,
    StockQuantity,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use shared::db::pagination_support::{PaginationDirection, PaginationParams, PaginationQuery};
use shared::dto_helpers::{fmt_datetime, fmt_datetime_opt, fmt_id};
use shared::errors::AppError;
use uuid::Uuid;
// ── Filter DTOs ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ProductFilterQuery {
    // Pagination fields
    pub limit: Option<u32>,
    pub cursor: Option<Uuid>,
    pub direction: Option<PaginationDirection>,
    // Filter fields
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
    pub min_price: Option<Decimal>,
    pub max_price: Option<Decimal>,
    pub search: Option<String>,
    pub status: Option<ProductStatus>,
}

impl ProductFilterQuery {
    pub fn into_parts(self) -> (PaginationParams, ProductFilter) {
        let pagination = PaginationQuery {
            limit: self.limit,
            cursor: self.cursor,
            direction: self.direction,
        };
        let search = self.search.and_then(|s| SearchQuery::new(&s).ok());
        let filter = ProductFilter {
            category_id: self.category_id,
            brand_id: self.brand_id,
            min_price: self.min_price,
            max_price: self.max_price,
            search,
            status: self.status,
        };
        (pagination.into_params(), filter)
    }
}

pub struct ProductFilter {
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
    pub min_price: Option<Decimal>,
    pub max_price: Option<Decimal>,
    pub search: Option<SearchQuery>,
    pub status: Option<ProductStatus>,
}

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
pub struct ProductListRes {
    pub id: String,
    pub created_at: String,
    pub seller_id: String,
    pub name: String,
    pub slug: String,
    pub base_price: Decimal,
    pub currency: String,
    pub category_id: Option<String>,
    pub brand_id: Option<String>,
    pub status: ProductStatus,
    pub category_name: Option<String>,
    pub brand_name: Option<String>,
    pub image_url: Option<String>,
}

impl ProductListRes {
    pub fn new(entity: ProductListEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            seller_id: fmt_id(&entity.seller_id),
            name: entity.name,
            slug: entity.slug,
            base_price: entity.base_price,
            currency: entity.currency,
            category_id: entity.category_id.map(|id| fmt_id(&id)),
            brand_id: entity.brand_id.map(|id| fmt_id(&id)),
            status: entity.status,
            category_name: entity.category_name,
            brand_name: entity.brand_name,
            image_url: entity.image_url,
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

impl ProductDetailRes {
    pub fn from_row(row: ProductDetailRow) -> Result<Self, AppError> {
        let product = ProductRes {
            id: fmt_id(&row.id),
            created_at: fmt_datetime(&row.created_at),
            updated_at: fmt_datetime_opt(&row.updated_at),
            seller_id: fmt_id(&row.seller_id),
            name: row.name,
            slug: row.slug,
            description: row.description,
            base_price: row.base_price,
            currency: row.currency,
            category_id: row.category_id.map(|id| fmt_id(&id)),
            brand_id: row.brand_id.map(|id| fmt_id(&id)),
            status: row.status,
            category_name: row.category_name,
            category_slug: row.category_slug,
            brand_name: row.brand_name,
            brand_slug: row.brand_slug,
        };

        let skus: Vec<SkuEntity> = serde_json::from_value(row.skus_json)
            .map_err(|e| AppError::InternalServerError(format!("Failed to parse SKUs: {e}")))?;
        let images: Vec<ProductImageEntity> = serde_json::from_value(row.images_json)
            .map_err(|e| AppError::InternalServerError(format!("Failed to parse images: {e}")))?;

        Ok(Self {
            product,
            skus: skus.into_iter().map(SkuRes::new).collect(),
            images: images.into_iter().map(ProductImageRes::new).collect(),
        })
    }
}

// ── Validated DTOs ──────────────────────────────────────────

pub struct ValidCreateProductReq {
    pub name: ProductName,
    pub slug: Slug,
    pub description: Option<String>,
    pub base_price: Price,
    pub currency: Currency,
    pub category_id: Option<CategoryId>,
    pub brand_id: Option<BrandId>,
}

impl ValidCreateProductReq {
    pub fn new(req: CreateProductReq) -> Result<Self, AppError> {
        let name = ProductName::new(&req.name)?;
        let slug = match req.slug {
            Some(s) => Slug::new(&s)?,
            None => Slug::from_name(name.as_str())?,
        };

        Ok(Self {
            name,
            slug,
            description: req.description,
            base_price: Price::new(req.base_price)?,
            currency: match req.currency {
                Some(c) => Currency::new(&c)?,
                None => Currency::default(),
            },
            category_id: req.category_id.map(CategoryId::new),
            brand_id: req.brand_id.map(BrandId::new),
        })
    }
}

pub struct ValidUpdateProductReq {
    pub name: Option<ProductName>,
    pub slug: Option<Slug>,
    pub description: Option<String>,
    pub base_price: Option<Price>,
    pub currency: Option<Currency>,
    pub category_id: Option<CategoryId>,
    pub brand_id: Option<BrandId>,
    pub status: Option<ProductStatus>,
}

impl ValidUpdateProductReq {
    pub fn new(req: UpdateProductReq, existing: &ProductEntity) -> Result<Self, AppError> {
        // Merge: use updated value if present, else keep existing.
        let effective_category = req
            .category_id
            .map(CategoryId::new)
            .or(existing.category_id.map(CategoryId::new));
        let effective_brand = req
            .brand_id
            .map(BrandId::new)
            .or(existing.brand_id.map(BrandId::new));
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
