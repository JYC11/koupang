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
        let name = CategoryName::new(&value.name)?;
        let slug = Slug::new(&value.slug)?;
        let label = LtreeLabel::from_name(name.as_str())?;
        Ok(Self {
            id: value.id,
            name,
            slug,
            path: label,
            parent_id: value.parent_id,
            depth: value.depth,
            description: value.description,
        })
    }
}
