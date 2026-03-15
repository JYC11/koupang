use crate::cart::domain::{Cart, CartItem};
use crate::cart::repository::CartItemStored;
use crate::cart::value_objects::{CartProductName, Currency, PriceSnapshot, Quantity};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use shared::dto_helpers::fmt_id;
use shared::errors::AppError;
use uuid::Uuid;

// ── Request DTOs ────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct AddToCartReq {
    pub product_id: Uuid,
    pub sku_id: Uuid,
    pub quantity: u32,
    pub unit_price: Decimal,
    pub currency: Option<String>,
    pub product_name: String,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateCartItemReq {
    pub quantity: u32,
}

// ── Validated DTOs ──────────────────────────────────────────

pub struct ValidAddToCartReq {
    pub product_id: Uuid,
    pub sku_id: Uuid,
    pub quantity: Quantity,
    pub unit_price: PriceSnapshot,
    pub currency: Currency,
    pub product_name: CartProductName,
    pub image_url: Option<String>,
}

impl TryFrom<AddToCartReq> for ValidAddToCartReq {
    type Error = AppError;

    fn try_from(req: AddToCartReq) -> Result<Self, Self::Error> {
        Ok(Self {
            product_id: req.product_id,
            sku_id: req.sku_id,
            quantity: Quantity::new(req.quantity)?,
            unit_price: PriceSnapshot::new(req.unit_price)?,
            currency: match req.currency {
                Some(c) => Currency::new(&c)?,
                None => Currency::default(),
            },
            product_name: CartProductName::new(&req.product_name)?,
            image_url: req.image_url,
        })
    }
}

impl ValidAddToCartReq {
    pub fn to_stored(&self) -> CartItemStored {
        CartItemStored {
            product_id: self.product_id,
            sku_id: self.sku_id,
            quantity: self.quantity.value(),
            unit_price: self.unit_price.value().to_string(),
            currency: self.currency.as_str().to_string(),
            product_name: self.product_name.as_str().to_string(),
            image_url: self.image_url.clone(),
            added_at: Utc::now(),
        }
    }
}

pub struct ValidUpdateCartItemReq {
    pub quantity: Quantity,
}

impl TryFrom<UpdateCartItemReq> for ValidUpdateCartItemReq {
    type Error = AppError;

    fn try_from(req: UpdateCartItemReq) -> Result<Self, Self::Error> {
        Ok(Self {
            quantity: Quantity::new(req.quantity)?,
        })
    }
}

// ── Response DTOs ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartRes {
    pub items: Vec<CartItemRes>,
    pub item_count: usize,
    pub total: Decimal,
}

impl CartRes {
    pub fn from_cart(cart: &Cart) -> Self {
        Self {
            items: cart.items.iter().map(CartItemRes::from_item).collect(),
            item_count: cart.item_count(),
            total: cart.total(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartItemRes {
    pub product_id: String,
    pub sku_id: String,
    pub quantity: u32,
    pub unit_price: Decimal,
    pub currency: String,
    pub product_name: String,
    pub image_url: Option<String>,
    pub line_total: Decimal,
}

impl CartItemRes {
    pub fn from_item(item: &CartItem) -> Self {
        Self {
            product_id: fmt_id(&item.product_id),
            sku_id: fmt_id(&item.sku_id),
            quantity: item.quantity.value(),
            unit_price: item.unit_price.value(),
            currency: item.currency.as_str().to_string(),
            product_name: item.product_name.clone(),
            image_url: item.image_url.clone(),
            line_total: item.line_total(),
        }
    }
}

// ── Validation Response (stub) ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartValidationRes {
    pub items: Vec<CartValidationItemRes>,
    pub all_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartValidationItemRes {
    pub sku_id: String,
    pub price_changed: bool,
    pub snapshot_price: Decimal,
    pub current_price: Option<Decimal>,
    pub product_unavailable: bool,
    pub stock_insufficient: bool,
}

// ── Conversion: stored → domain ─────────────────────────────

impl CartItemStored {
    pub fn to_domain(&self) -> Result<CartItem, AppError> {
        let unit_price: Decimal = self
            .unit_price
            .parse()
            .map_err(|e| AppError::InternalServerError(format!("Invalid stored price: {e}")))?;

        Ok(CartItem {
            product_id: self.product_id,
            sku_id: self.sku_id,
            quantity: Quantity::new(self.quantity)?,
            unit_price: PriceSnapshot::new(unit_price)?,
            currency: Currency::new(&self.currency)?,
            product_name: self.product_name.clone(),
            image_url: self.image_url.clone(),
            added_at: self.added_at,
        })
    }
}
