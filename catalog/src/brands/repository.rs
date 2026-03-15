use crate::brands::dtos::{ValidCreateBrandReq, ValidUpdateBrandReq};
use crate::brands::entities::BrandEntity;
use crate::brands::value_objects::BrandId;
use crate::categories::entities::CategoryEntity;
use crate::categories::value_objects::CategoryId;
use shared::db::PgExec;
use shared::db::pagination_support::{PaginationParams, keyset_paginate};
use shared::errors::AppError;
use sqlx::postgres::Postgres;
use sqlx::{PgConnection, QueryBuilder};

// ── Brand queries ──────────────────────────────────────────

pub async fn get_brand_by_id<'e>(
    executor: impl PgExec<'e>,
    id: BrandId,
) -> Result<BrandEntity, AppError> {
    sqlx::query_as::<_, BrandEntity>("SELECT * FROM brands WHERE id = $1")
        .bind(id.value())
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("Brand not found: {}", e)))
}

pub async fn get_brand_by_slug<'e>(
    executor: impl PgExec<'e>,
    slug: &str,
) -> Result<BrandEntity, AppError> {
    sqlx::query_as::<_, BrandEntity>("SELECT * FROM brands WHERE slug = $1")
        .bind(slug)
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("Brand not found: {}", e)))
}

pub async fn list_brands<'e>(
    executor: impl PgExec<'e>,
    params: &PaginationParams,
) -> Result<Vec<BrandEntity>, AppError> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM brands WHERE 1=1");
    keyset_paginate(params, None, &mut qb);
    qb.build_query_as::<BrandEntity>()
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to list brands: {}", e)))
}

pub async fn create_brand(
    tx: &mut PgConnection,
    req: ValidCreateBrandReq,
) -> Result<BrandId, AppError> {
    let row: (uuid::Uuid,) = sqlx::query_as(
        "INSERT INTO brands (name, slug, description, logo_url)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
    )
    .bind(req.name.as_str())
    .bind(req.slug.as_str())
    .bind(&req.description)
    .bind(req.logo_url.as_ref().map(|u| u.as_str()))
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create brand: {}", e)))?;

    Ok(BrandId::new(row.0))
}

pub async fn update_brand(
    tx: &mut PgConnection,
    id: BrandId,
    req: ValidUpdateBrandReq,
) -> Result<(), AppError> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE brands SET ");
    let mut fields = 0usize;
    {
        let mut sep = qb.separated(", ");
        if let Some(ref name) = req.name {
            sep.push("name = ").push_bind_unseparated(name.as_str());
            fields += 1;
        }
        if let Some(ref description) = req.description {
            sep.push("description = ")
                .push_bind_unseparated(description.clone());
            fields += 1;
        }
        if let Some(ref logo_url) = req.logo_url {
            sep.push("logo_url = ")
                .push_bind_unseparated(logo_url.as_str());
            fields += 1;
        }
        if fields == 0 {
            return Ok(());
        }
        sep.push("updated_at = NOW()");
    }
    qb.push(" WHERE id = ").push_bind(id.value());

    let result = qb
        .build()
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update brand: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Brand not found".to_string()));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "UPDATE must affect exactly 1 row"
    );

    Ok(())
}

pub async fn delete_brand(tx: &mut PgConnection, id: BrandId) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM brands WHERE id = $1")
        .bind(id.value())
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete brand: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Brand not found".to_string()));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "DELETE must affect exactly 1 row"
    );

    Ok(())
}

/// Check if any products reference this brand.
pub async fn has_products<'e>(executor: impl PgExec<'e>, id: BrandId) -> Result<bool, AppError> {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM products WHERE brand_id = $1 AND deleted_at IS NULL")
            .bind(id.value())
            .fetch_one(executor)
            .await
            .map_err(|e| {
                AppError::InternalServerError(format!("Failed to check products: {}", e))
            })?;

    Ok(row.0 > 0)
}

// ── Brand-Category association queries ─────────────────────

pub async fn associate_category(
    tx: &mut PgConnection,
    brand_id: BrandId,
    category_id: CategoryId,
) -> Result<(), AppError> {
    let result = sqlx::query(
        "INSERT INTO brand_categories (brand_id, category_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(brand_id.value())
    .bind(category_id.value())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to associate category: {}", e)))?;

    debug_assert!(
        result.rows_affected() <= 1,
        "INSERT ON CONFLICT must affect 0 or 1 rows"
    );
    Ok(())
}

pub async fn disassociate_category(
    tx: &mut PgConnection,
    brand_id: BrandId,
    category_id: CategoryId,
) -> Result<(), AppError> {
    let result =
        sqlx::query("DELETE FROM brand_categories WHERE brand_id = $1 AND category_id = $2")
            .bind(brand_id.value())
            .bind(category_id.value())
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                AppError::InternalServerError(format!("Failed to disassociate category: {}", e))
            })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(
            "Brand-category association not found".to_string(),
        ));
    }

    Ok(())
}

/// List categories associated with a brand.
pub async fn list_categories_for_brand<'e>(
    executor: impl PgExec<'e>,
    brand_id: BrandId,
    params: &PaginationParams,
) -> Result<Vec<CategoryEntity>, AppError> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "SELECT c.id, c.created_at, c.updated_at, c.name, c.slug, c.path::text as path,
                c.parent_id, c.depth, c.description
         FROM categories c
         INNER JOIN brand_categories bc ON bc.category_id = c.id
         WHERE bc.brand_id = ",
    );
    qb.push_bind(brand_id.value());
    keyset_paginate(params, Some("c"), &mut qb);
    qb.build_query_as::<CategoryEntity>()
        .fetch_all(executor)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to list categories for brand: {}", e))
        })
}
