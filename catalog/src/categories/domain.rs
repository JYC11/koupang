use crate::categories::entities::CategoryEntity;
use crate::categories::value_objects::{CategoryName, LtreeLabel};
use crate::common::value_objects::Slug;
use shared::errors::AppError;
use uuid::Uuid;

pub struct Category {
    pub id: Uuid,
    pub name: CategoryName,
    pub slug: Slug,
    pub path: LtreeLabel,
    pub parent_id: Option<Uuid>,
    pub depth: i32,
    pub description: Option<String>,
}

impl TryFrom<CategoryEntity> for Category {
    type Error = AppError;

    fn try_from(value: CategoryEntity) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            name: CategoryName::new(&value.name)?,
            slug: Slug::new(&value.slug)?,
            path: LtreeLabel::from_name(value.path.as_str())?,
            parent_id: value.parent_id,
            depth: value.depth,
            description: value.description,
        })
    }
}
