use crate::products::entities::{ProductEntity, SkuEntity};
use crate::products::value_objects::{
    Currency, Price, ProductName, ProductStatus, SkuCode, Slug, StockQuantity,
};
use shared::errors::AppError;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Product {
    pub id: Uuid,
    pub name: ProductName,
    pub slug: Slug,
    pub description: Option<String>,
    pub base_price: Price,
    pub currency: Currency,
    pub category_id: Option<Uuid>,
    pub brand_id: Option<Uuid>,
    pub status: ProductStatus,
}

impl TryFrom<ProductEntity> for Product {
    type Error = AppError;

    fn try_from(value: ProductEntity) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            name: ProductName::new(&*value.name)?,
            slug: Slug::new(&*value.slug)?,
            description: value.description,
            base_price: Price::new(value.base_price)?,
            currency: Currency::new(&*value.currency)?,
            category_id: value.category_id,
            brand_id: value.brand_id,
            status: value.status,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Sku {
    pub id: Uuid,
    pub product_id: Uuid,
    pub sku_code: SkuCode,
    pub price: Price,
    pub stock_quantity: StockQuantity,
    pub attributes: serde_json::Value,
}

impl TryFrom<(Uuid, SkuEntity)> for Sku {
    type Error = AppError;

    fn try_from(value: (Uuid, SkuEntity)) -> Result<Self, Self::Error> {
        let (product_id, entity) = value;
        Ok(Self {
            id: entity.id,
            product_id,
            sku_code: SkuCode::new(&*entity.sku_code)?,
            price: Price::new(entity.price)?,
            stock_quantity: StockQuantity::new(entity.stock_quantity)?,
            attributes: entity.attributes,
        })
    }
}
