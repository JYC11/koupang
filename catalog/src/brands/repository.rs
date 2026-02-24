use crate::brands::dtos::{ValidCreateBrandReq, ValidUpdateBrandReq};
use crate::brands::entities::BrandEntity;
use crate::categories::entities::CategoryEntity;
use shared::db::PgExec;
use shared::errors::AppError;
use sqlx::PgConnection;
use uuid::Uuid;

// ── Brand queries ──────────────────────────────────────────

pub async fn get_brand_by_id<'e>(
    executor: impl PgExec<'e>,
    id: Uuid,
) -> Result<BrandEntity, AppError> {
    sqlx::query_as::<_, BrandEntity>("SELECT * FROM brands WHERE id = $1")
        .bind(id)
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

pub async fn list_brands<'e>(executor: impl PgExec<'e>) -> Result<Vec<BrandEntity>, AppError> {
    sqlx::query_as::<_, BrandEntity>("SELECT * FROM brands ORDER BY name ASC")
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to list brands: {}", e)))
}

pub async fn create_brand(
    tx: &mut PgConnection,
    req: ValidCreateBrandReq,
) -> Result<Uuid, AppError> {
    let row: (Uuid,) = sqlx::query_as(
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

    Ok(row.0)
}

pub async fn update_brand(
    tx: &mut PgConnection,
    id: Uuid,
    req: ValidUpdateBrandReq,
) -> Result<(), AppError> {
    let mut set_parts: Vec<String> = Vec::new();
    let mut param_idx = 2u32;

    if req.name.is_some() {
        set_parts.push(format!("name = ${}", param_idx));
        param_idx += 1;
    }
    if req.description.is_some() {
        set_parts.push(format!("description = ${}", param_idx));
        param_idx += 1;
    }
    if req.logo_url.is_some() {
        set_parts.push(format!("logo_url = ${}", param_idx));
        param_idx += 1;
    }

    if set_parts.is_empty() {
        return Ok(());
    }

    set_parts.push("updated_at = NOW()".to_string());
    let _ = param_idx;

    let sql = format!("UPDATE brands SET {} WHERE id = $1", set_parts.join(", "));

    let mut query = sqlx::query(&sql).bind(id);

    if let Some(ref name) = req.name {
        query = query.bind(name.as_str());
    }
    if let Some(ref description) = req.description {
        query = query.bind(description);
    }
    if let Some(ref logo_url) = req.logo_url {
        query = query.bind(logo_url.as_str());
    }

    let result = query
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update brand: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Brand not found".to_string()));
    }

    Ok(())
}

pub async fn delete_brand(tx: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM brands WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete brand: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Brand not found".to_string()));
    }

    Ok(())
}

/// Check if any products reference this brand.
pub async fn has_products<'e>(executor: impl PgExec<'e>, id: Uuid) -> Result<bool, AppError> {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM products WHERE brand_id = $1 AND deleted_at IS NULL")
            .bind(id)
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
    brand_id: Uuid,
    category_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO brand_categories (brand_id, category_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
        .bind(brand_id)
        .bind(category_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to associate category: {}", e)))?;

    Ok(())
}

pub async fn disassociate_category(
    tx: &mut PgConnection,
    brand_id: Uuid,
    category_id: Uuid,
) -> Result<(), AppError> {
    let result =
        sqlx::query("DELETE FROM brand_categories WHERE brand_id = $1 AND category_id = $2")
            .bind(brand_id)
            .bind(category_id)
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
    brand_id: Uuid,
) -> Result<Vec<CategoryEntity>, AppError> {
    sqlx::query_as::<_, CategoryEntity>(
        "SELECT c.id, c.created_at, c.updated_at, c.name, c.slug, c.path::text as path,
                c.parent_id, c.depth, c.description
         FROM categories c
         INNER JOIN brand_categories bc ON bc.category_id = c.id
         WHERE bc.brand_id = $1
         ORDER BY c.name ASC",
    )
        .bind(brand_id)
        .fetch_all(executor)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to list categories for brand: {}", e))
        })
}
