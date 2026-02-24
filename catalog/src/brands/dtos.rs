use crate::brands::entities::BrandEntity;
use crate::brands::value_objects::BrandName;
use crate::common::value_objects::{HttpUrl, Slug};
use serde::{Deserialize, Serialize};
use shared::dto_helpers::{fmt_datetime, fmt_datetime_opt, fmt_id};
use shared::errors::AppError;
use uuid::Uuid;

// ── Request DTOs ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBrandReq {
    pub name: String,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateBrandReq {
    pub name: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssociateCategoryReq {
    pub category_id: Uuid,
}

// ── Response DTOs ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandRes {
    pub id: String,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub logo_url: Option<String>,
}

impl BrandRes {
    pub fn new(entity: BrandEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            updated_at: fmt_datetime_opt(&entity.updated_at),
            name: entity.name,
            slug: entity.slug,
            description: entity.description,
            logo_url: entity.logo_url,
        }
    }
}

// ── Validated DTOs ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ValidCreateBrandReq {
    pub name: BrandName,
    pub slug: Slug,
    pub description: Option<String>,
    pub logo_url: Option<HttpUrl>,
}

impl TryFrom<CreateBrandReq> for ValidCreateBrandReq {
    type Error = AppError;

    fn try_from(req: CreateBrandReq) -> Result<Self, Self::Error> {
        let name = BrandName::new(&req.name)?;
        let slug = match req.slug {
            Some(s) => Slug::new(&s)?,
            None => Slug::from_name(name.as_str())?,
        };
        let logo_url = req.logo_url.map(|u| HttpUrl::new(&u)).transpose()?;

        Ok(Self {
            name,
            slug,
            description: req.description,
            logo_url,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ValidUpdateBrandReq {
    pub name: Option<BrandName>,
    pub description: Option<String>,
    pub logo_url: Option<HttpUrl>,
}

impl TryFrom<UpdateBrandReq> for ValidUpdateBrandReq {
    type Error = AppError;

    fn try_from(req: UpdateBrandReq) -> Result<Self, Self::Error> {
        let name = req.name.map(|n| BrandName::new(&n)).transpose()?;
        let logo_url = req.logo_url.map(|u| HttpUrl::new(&u)).transpose()?;
        Ok(Self {
            name,
            description: req.description,
            logo_url,
        })
    }
}
