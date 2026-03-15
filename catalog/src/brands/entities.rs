use shared::db::pagination_support::HasId;
use sqlx::FromRow;
use sqlx::types::Uuid;
use sqlx::types::chrono::{DateTime, Utc};

#[derive(Debug, Clone, FromRow)]
pub struct BrandEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub logo_url: Option<String>,
}

impl HasId for BrandEntity {
    fn id(&self) -> Uuid {
        self.id
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct BrandCategoryEntity {
    pub brand_id: Uuid,
    pub category_id: Uuid,
}
