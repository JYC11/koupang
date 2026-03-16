use rust_decimal::Decimal;
use serde::Deserialize;
use shared::db::pagination_support::HasId;
use sqlx::FromRow;
use sqlx::types::Uuid;
use sqlx::types::chrono::{DateTime, Utc};

use super::value_objects::{ProductStatus, SkuStatus};

#[derive(Debug, Clone, FromRow)]
pub struct ProductEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub seller_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub base_price: Decimal,
    pub currency: String,
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
    pub status: ProductStatus,
    // Populated by LEFT JOINs (NULL when no FK set)
    pub category_name: Option<String>,
    pub category_slug: Option<String>,
    pub brand_name: Option<String>,
    pub brand_slug: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ProductListEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub seller_id: Uuid,
    pub name: String,
    pub slug: String,
    pub base_price: Decimal,
    pub currency: String,
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
    pub status: ProductStatus,
    pub category_name: Option<String>,
    pub brand_name: Option<String>,
    pub image_url: Option<String>,
}

impl HasId for ProductListEntity {
    fn id(&self) -> Uuid {
        self.id
    }
}

#[derive(Debug, Clone, FromRow, Deserialize)]
pub struct SkuEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub product_id: Uuid,
    pub sku_code: String,
    pub price: Decimal,
    pub stock_quantity: i32,
    pub reserved_quantity: i32,
    pub attributes: serde_json::Value,
    pub status: SkuStatus,
}

#[derive(Debug, Clone, FromRow, Deserialize)]
pub struct ProductImageEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub product_id: Uuid,
    pub url: String,
    pub alt_text: Option<String>,
    pub sort_order: i32,
    pub is_primary: bool,
}

/// Combined product + SKUs + images from a single LATERAL JOIN query.
/// SKUs and images are aggregated as JSON arrays by the database.
#[derive(Debug, Clone, FromRow)]
pub struct ProductDetailRow {
    // Product fields (same as ProductEntity)
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub seller_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub base_price: Decimal,
    pub currency: String,
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
    pub status: ProductStatus,
    pub category_name: Option<String>,
    pub category_slug: Option<String>,
    pub brand_name: Option<String>,
    pub brand_slug: Option<String>,
    // Aggregated JSON from LATERAL subqueries
    pub skus_json: serde_json::Value,
    pub images_json: serde_json::Value,
}
