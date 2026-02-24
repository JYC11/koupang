use crate::categories::entities::CategoryEntity;
use crate::categories::value_objects::{CategoryName, LtreeLabel};
use crate::common::value_objects::Slug;
use serde::{Deserialize, Serialize};
use shared::dto_helpers::{fmt_datetime, fmt_datetime_opt, fmt_id};
use shared::errors::AppError;
use uuid::Uuid;

// ── Request DTOs ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCategoryReq {
    pub name: String,
    pub slug: Option<String>,
    pub parent_id: Option<Uuid>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCategoryReq {
    pub name: Option<String>,
    pub description: Option<String>,
}

// ── Response DTOs ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRes {
    pub id: String,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub name: String,
    pub slug: String,
    pub path: String,
    pub parent_id: Option<String>,
    pub depth: i32,
    pub description: Option<String>,
}

impl CategoryRes {
    pub fn new(entity: CategoryEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            updated_at: fmt_datetime_opt(&entity.updated_at),
            name: entity.name,
            slug: entity.slug,
            path: entity.path,
            parent_id: entity.parent_id.map(|id| fmt_id(&id)),
            depth: entity.depth,
            description: entity.description,
        }
    }
}

// ── Validated DTOs ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ValidCreateCategoryReq {
    pub name: CategoryName,
    pub slug: Slug,
    pub label: LtreeLabel,
    pub parent_id: Option<Uuid>,
    pub description: Option<String>,
}

impl TryFrom<CreateCategoryReq> for ValidCreateCategoryReq {
    type Error = AppError;

    fn try_from(req: CreateCategoryReq) -> Result<Self, Self::Error> {
        let name = CategoryName::new(&req.name)?;
        let slug = match req.slug {
            Some(s) => Slug::new(&s)?,
            None => Slug::from_name(name.as_str())?,
        };
        let label = LtreeLabel::from_name(name.as_str())?;

        Ok(Self {
            name,
            slug,
            label,
            parent_id: req.parent_id,
            description: req.description,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ValidUpdateCategoryReq {
    pub name: Option<CategoryName>,
    pub description: Option<String>,
}

impl TryFrom<UpdateCategoryReq> for ValidUpdateCategoryReq {
    type Error = AppError;

    fn try_from(req: UpdateCategoryReq) -> Result<Self, Self::Error> {
        let name = req.name.map(|n| CategoryName::new(&n)).transpose()?;
        Ok(Self {
            name,
            description: req.description,
        })
    }
}
