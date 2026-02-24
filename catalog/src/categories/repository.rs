use crate::categories::dtos::{ValidCreateCategoryReq, ValidUpdateCategoryReq};
use crate::categories::entities::CategoryEntity;
use shared::db::PgExec;
use shared::errors::AppError;
use sqlx::PgConnection;
use uuid::Uuid;

pub async fn get_category_by_id<'e>(
    executor: impl PgExec<'e>,
    id: Uuid,
) -> Result<CategoryEntity, AppError> {
    sqlx::query_as::<_, CategoryEntity>(
        "SELECT id, created_at, updated_at, name, slug, path::text as path, parent_id, depth, description
         FROM categories WHERE id = $1",
    )
        .bind(id)
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("Category not found: {}", e)))
}

pub async fn get_category_by_slug<'e>(
    executor: impl PgExec<'e>,
    slug: &str,
) -> Result<CategoryEntity, AppError> {
    sqlx::query_as::<_, CategoryEntity>(
        "SELECT id, created_at, updated_at, name, slug, path::text as path, parent_id, depth, description
         FROM categories WHERE slug = $1",
    )
        .bind(slug)
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("Category not found: {}", e)))
}

pub async fn list_root_categories<'e>(
    executor: impl PgExec<'e>,
) -> Result<Vec<CategoryEntity>, AppError> {
    sqlx::query_as::<_, CategoryEntity>(
        "SELECT id, created_at, updated_at, name, slug, path::text as path, parent_id, depth, description
         FROM categories WHERE parent_id IS NULL ORDER BY name ASC",
    )
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to list root categories: {}", e)))
}

pub async fn get_children<'e>(
    executor: impl PgExec<'e>,
    parent_id: Uuid,
) -> Result<Vec<CategoryEntity>, AppError> {
    sqlx::query_as::<_, CategoryEntity>(
        "SELECT id, created_at, updated_at, name, slug, path::text as path, parent_id, depth, description
         FROM categories WHERE parent_id = $1 ORDER BY name ASC",
    )
        .bind(parent_id)
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to list children: {}", e)))
}

/// Get all descendants of a category (including itself) using ltree <@ operator.
pub async fn get_subtree<'e>(
    executor: impl PgExec<'e>,
    path: &str,
) -> Result<Vec<CategoryEntity>, AppError> {
    sqlx::query_as::<_, CategoryEntity>(
        "SELECT id, created_at, updated_at, name, slug, path::text as path, parent_id, depth, description
         FROM categories WHERE path <@ $1::ltree ORDER BY path ASC",
    )
        .bind(path)
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to get subtree: {}", e)))
}

/// Get all ancestors of a category (including itself) using ltree @> operator.
pub async fn get_ancestors<'e>(
    executor: impl PgExec<'e>,
    path: &str,
) -> Result<Vec<CategoryEntity>, AppError> {
    sqlx::query_as::<_, CategoryEntity>(
        "SELECT id, created_at, updated_at, name, slug, path::text as path, parent_id, depth, description
         FROM categories WHERE path @> $1::ltree ORDER BY depth ASC",
    )
        .bind(path)
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to get ancestors: {}", e)))
}

pub async fn create_category(
    tx: &mut PgConnection,
    req: ValidCreateCategoryReq,
    path: &str,
    depth: i32,
) -> Result<Uuid, AppError> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO categories (name, slug, path, parent_id, depth, description)
         VALUES ($1, $2, $3::ltree, $4, $5, $6)
         RETURNING id",
    )
        .bind(req.name.as_str())
        .bind(req.slug.as_str())
        .bind(path)
        .bind(&req.parent_id)
        .bind(depth)
        .bind(&req.description)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to create category: {}", e)))?;

    Ok(row.0)
}

pub async fn update_category(
    tx: &mut PgConnection,
    id: Uuid,
    req: ValidUpdateCategoryReq,
) -> Result<(), AppError> {
    let mut set_parts: Vec<String> = Vec::new();
    let mut param_idx = 2u32; // $1 is the id

    if req.name.is_some() {
        set_parts.push(format!("name = ${}", param_idx));
        param_idx += 1;
    }
    if req.description.is_some() {
        set_parts.push(format!("description = ${}", param_idx));
        param_idx += 1;
    }

    if set_parts.is_empty() {
        return Ok(());
    }

    set_parts.push("updated_at = NOW()".to_string());
    let _ = param_idx;

    let sql = format!(
        "UPDATE categories SET {} WHERE id = $1",
        set_parts.join(", ")
    );

    let mut query = sqlx::query(&sql).bind(id);

    if let Some(ref name) = req.name {
        query = query.bind(name.as_str());
    }
    if let Some(ref description) = req.description {
        query = query.bind(description);
    }

    let result = query
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update category: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Category not found".to_string()));
    }

    Ok(())
}

pub async fn delete_category(tx: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM categories WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete category: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Category not found".to_string()));
    }

    Ok(())
}

/// Check if a category has child categories.
pub async fn has_children<'e>(executor: impl PgExec<'e>, id: Uuid) -> Result<bool, AppError> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM categories WHERE parent_id = $1")
        .bind(id)
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to check children: {}", e)))?;

    Ok(row.0 > 0)
}

/// Check if any products reference this category.
pub async fn has_products<'e>(executor: impl PgExec<'e>, id: Uuid) -> Result<bool, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM products WHERE category_id = $1 AND deleted_at IS NULL",
    )
        .bind(id)
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to check products: {}", e)))?;

    Ok(row.0 > 0)
}
