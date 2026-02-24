use crate::brands::entities::BrandEntity;
use crate::brands::value_objects::{BrandName, HttpUrl};
use crate::products::value_objects::Slug;
use shared::errors::AppError;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Brand {
    pub id: Uuid,
    pub name: BrandName,
    pub slug: Slug,
    pub description: Option<String>,
    pub logo_url: Option<HttpUrl>,
}

impl TryFrom<BrandEntity> for Brand {
    type Error = AppError;

    fn try_from(value: BrandEntity) -> Result<Self, Self::Error> {
        let name = BrandName::new(&*value.name)?;
        let slug = Slug::new(&*value.slug)?;
        let logo_url = value.logo_url.map(|u| HttpUrl::new(&u)).transpose()?;
        Ok(Self {
            id: value.id,
            name,
            slug,
            description: value.description,
            logo_url,
        })
    }
}
