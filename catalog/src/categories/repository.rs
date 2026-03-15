use crate::categories::dtos::{ValidCreateCategoryReq, ValidUpdateCategoryReq};
use crate::categories::entities::CategoryEntity;
use crate::categories::value_objects::CategoryId;
use shared::db::PgExec;
use shared::db::pagination_support::{PaginationParams, keyset_paginate};
use shared::errors::AppError;
use sqlx::postgres::Postgres;
use sqlx::{PgConnection, QueryBuilder};

pub async fn get_category_by_id<'e>(
    executor: impl PgExec<'e>,
    id: CategoryId,
) -> Result<CategoryEntity, AppError> {
    sqlx::query_as::<_, CategoryEntity>(
        "SELECT id, created_at, updated_at, name, slug, path::text as path, parent_id, depth, description
         FROM categories WHERE id = $1",
    )
        .bind(id.value())
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
    params: &PaginationParams,
) -> Result<Vec<CategoryEntity>, AppError> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "SELECT id, created_at, updated_at, name, slug, path::text as path, parent_id, depth, description
         FROM categories WHERE parent_id IS NULL",
    );
    keyset_paginate(params, None, &mut qb);
    qb.build_query_as::<CategoryEntity>()
        .fetch_all(executor)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to list root categories: {}", e))
        })
}

pub async fn get_children<'e>(
    executor: impl PgExec<'e>,
    parent_id: CategoryId,
    params: &PaginationParams,
) -> Result<Vec<CategoryEntity>, AppError> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "SELECT id, created_at, updated_at, name, slug, path::text as path, parent_id, depth, description
         FROM categories WHERE parent_id = ",
    );
    qb.push_bind(parent_id.value());
    keyset_paginate(params, None, &mut qb);
    qb.build_query_as::<CategoryEntity>()
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
         FROM categories WHERE path <@ $1::ltree ORDER BY path ASC LIMIT 200",
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
         FROM categories WHERE path @> $1::ltree ORDER BY depth ASC LIMIT 50",
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
) -> Result<CategoryId, AppError> {
    debug_assert!(depth >= 0, "Category depth must be non-negative");
    debug_assert_eq!(
        path.matches('.').count() as i32,
        depth,
        "Depth must equal number of dots in ltree path"
    );

    let row: (uuid::Uuid,) = sqlx::query_as(
        "INSERT INTO categories (name, slug, path, parent_id, depth, description)
         VALUES ($1, $2, $3::ltree, $4, $5, $6)
         RETURNING id",
    )
    .bind(req.name.as_str())
    .bind(req.slug.as_str())
    .bind(path)
    .bind(req.parent_id.map(|id| id.value()))
    .bind(depth)
    .bind(&req.description)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create category: {}", e)))?;

    Ok(CategoryId::new(row.0))
}

pub async fn update_category(
    tx: &mut PgConnection,
    id: CategoryId,
    req: ValidUpdateCategoryReq,
) -> Result<(), AppError> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE categories SET ");
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
        if fields == 0 {
            return Ok(());
        }
        sep.push("updated_at = NOW()");
    }
    qb.push(" WHERE id = ").push_bind(id.value());

    let result =
        qb.build().execute(&mut *tx).await.map_err(|e| {
            AppError::InternalServerError(format!("Failed to update category: {}", e))
        })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Category not found".to_string()));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "UPDATE must affect exactly 1 row"
    );

    Ok(())
}

pub async fn delete_category(tx: &mut PgConnection, id: CategoryId) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM categories WHERE id = $1")
        .bind(id.value())
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete category: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Category not found".to_string()));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "DELETE must affect exactly 1 row"
    );

    Ok(())
}

/// Check if a category has child categories.
pub async fn has_children<'e>(executor: impl PgExec<'e>, id: CategoryId) -> Result<bool, AppError> {
    let row: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM categories WHERE parent_id = $1)")
            .bind(id.value())
            .fetch_one(executor)
            .await
            .map_err(|e| {
                AppError::InternalServerError(format!("Failed to check children: {}", e))
            })?;

    Ok(row.0)
}

/// Check if any products reference this category.
pub async fn has_products<'e>(executor: impl PgExec<'e>, id: CategoryId) -> Result<bool, AppError> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM products WHERE category_id = $1 AND deleted_at IS NULL)",
    )
    .bind(id.value())
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to check products: {}", e)))?;

    Ok(row.0)
}
