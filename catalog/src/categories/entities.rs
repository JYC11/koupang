use shared::db::pagination_support::HasId;
use sqlx::FromRow;
use sqlx::types::Uuid;
use sqlx::types::chrono::{DateTime, Utc};

#[derive(Debug, Clone, FromRow)]
pub struct CategoryEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub name: String,
    pub slug: String,
    pub path: String, // ltree stored as text (cast in SQL)
    pub parent_id: Option<Uuid>,
    pub depth: i32,
    pub description: Option<String>,
}

impl HasId for CategoryEntity {
    fn id(&self) -> Uuid {
        self.id
    }
}
