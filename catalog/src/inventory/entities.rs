use sqlx::FromRow;
use sqlx::types::Uuid;
use sqlx::types::chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum ReservationStatus {
    Reserved,
    Released,
    Confirmed,
}

impl ReservationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Released => "released",
            Self::Confirmed => "confirmed",
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct InventoryReservationEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub order_id: Uuid,
    pub sku_id: Uuid,
    pub quantity: i32,
    pub status: ReservationStatus,
    pub released_at: Option<DateTime<Utc>>,
    pub confirmed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct SkuAvailabilityRow {
    pub sku_id: Uuid,
    pub product_id: Uuid,
    pub sku_code: String,
    pub stock_quantity: i32,
    pub reserved_quantity: i32,
    pub available_quantity: i32,
}
