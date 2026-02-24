use crate::products::entities::{ProductEntity, ProductImageEntity, SkuEntity};
use crate::products::value_objects::{
    Currency, ImageUrl, Price, ProductName, ProductStatus, SkuCode, SkuStatus, Slug,
    StockQuantity,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use shared::dto_helpers::{fmt_datetime, fmt_datetime_opt, fmt_id};
use shared::errors::AppError;

// ── Product Request DTOs ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProductReq {
    pub name: String,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub base_price: Decimal,
    pub currency: Option<String>,
    pub category: Option<String>,
    pub brand: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProductReq {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub base_price: Option<Decimal>,
    pub currency: Option<String>,
    pub category: Option<String>,
    pub brand: Option<String>,
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
    pub category: Option<String>,
    pub brand: Option<String>,
    pub status: ProductStatus,
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
            category: entity.category,
            brand: entity.brand,
            status: entity.status,
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

#[derive(Debug, Clone)]
pub struct ValidCreateProductReq {
    pub name: ProductName,
    pub slug: Slug,
    pub description: Option<String>,
    pub base_price: Price,
    pub currency: Currency,
    pub category: Option<String>,
    pub brand: Option<String>,
}

impl TryFrom<CreateProductReq> for ValidCreateProductReq {
    type Error = AppError;

    fn try_from(req: CreateProductReq) -> Result<Self, Self::Error> {
        let name = ProductName::new(&req.name)?;
        let slug = match req.slug {
            Some(s) => Slug::new(&s)?,
            None => Slug::from_name(name.as_str())?,
        };
        let base_price = Price::new(req.base_price)?;
        let currency = match req.currency {
            Some(c) => Currency::new(&c)?,
            None => Currency::default(),
        };

        Ok(Self {
            name,
            slug,
            description: req.description,
            base_price,
            currency,
            category: req.category,
            brand: req.brand,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ValidUpdateProductReq {
    pub name: Option<ProductName>,
    pub slug: Option<Slug>,
    pub description: Option<String>,
    pub base_price: Option<Price>,
    pub currency: Option<Currency>,
    pub category: Option<String>,
    pub brand: Option<String>,
    pub status: Option<ProductStatus>,
}

impl TryFrom<UpdateProductReq> for ValidUpdateProductReq {
    type Error = AppError;

    fn try_from(req: UpdateProductReq) -> Result<Self, Self::Error> {
        let name = req.name.map(|n| ProductName::new(&n)).transpose()?;
        let slug = req.slug.map(|s| Slug::new(&s)).transpose()?;
        let base_price = req.base_price.map(Price::new).transpose()?;
        let currency = req.currency.map(|c| Currency::new(&c)).transpose()?;

        Ok(Self {
            name,
            slug,
            description: req.description,
            base_price,
            currency,
            category: req.category,
            brand: req.brand,
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
