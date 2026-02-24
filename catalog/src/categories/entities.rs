use sqlx::types::chrono::{DateTime, Utc};
use sqlx::types::Uuid;
use sqlx::FromRow;

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
