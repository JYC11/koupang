use crate::AppState;
use crate::brands::dtos::{
    BrandRes, CreateBrandReq, UpdateBrandReq, ValidCreateBrandReq, ValidUpdateBrandReq,
};
use crate::brands::repository;
use crate::brands::value_objects::BrandId;
use crate::categories::dtos::CategoryRes;
use crate::categories::repository as category_repo;
use crate::categories::value_objects::CategoryId;
use shared::auth::guards::require_admin;
use shared::auth::jwt::CurrentUser;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::errors::AppError;

pub async fn create_brand(
    state: &AppState,
    current_user: &CurrentUser,
    req: CreateBrandReq,
) -> Result<BrandRes, AppError> {
    require_admin(current_user)?;

    let validated: ValidCreateBrandReq = req.try_into()?;

    let brand_id = with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::create_brand(tx.as_executor(), validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create brand: {}", e)))?;

    let brand = repository::get_brand_by_id(&state.pool, brand_id).await?;
    Ok(BrandRes::new(brand))
}

pub async fn get_brand(state: &AppState, id: BrandId) -> Result<BrandRes, AppError> {
    let brand = repository::get_brand_by_id(&state.pool, id).await?;
    Ok(BrandRes::new(brand))
}

pub async fn get_brand_by_slug(state: &AppState, slug: &str) -> Result<BrandRes, AppError> {
    let brand = repository::get_brand_by_slug(&state.pool, slug).await?;
    Ok(BrandRes::new(brand))
}

pub async fn list_brands(state: &AppState) -> Result<Vec<BrandRes>, AppError> {
    let brands = repository::list_brands(&state.pool).await?;
    Ok(brands.into_iter().map(BrandRes::new).collect())
}

pub async fn update_brand(
    state: &AppState,
    current_user: &CurrentUser,
    id: BrandId,
    req: UpdateBrandReq,
) -> Result<(), AppError> {
    require_admin(current_user)?;
    repository::get_brand_by_id(&state.pool, id).await?;

    let validated: ValidUpdateBrandReq = req.try_into()?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::update_brand(tx.as_executor(), id, validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to update brand: {}", e)))?;

    Ok(())
}

pub async fn delete_brand(
    state: &AppState,
    current_user: &CurrentUser,
    id: BrandId,
) -> Result<(), AppError> {
    require_admin(current_user)?;
    repository::get_brand_by_id(&state.pool, id).await?;

    if repository::has_products(&state.pool, id).await? {
        return Err(AppError::BadRequest(
            "Cannot delete brand with associated products. Reassign products first.".to_string(),
        ));
    }

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::delete_brand(tx.as_executor(), id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to delete brand: {}", e)))?;

    Ok(())
}

// ── Brand-Category associations ────────────────────────

pub async fn associate_category(
    state: &AppState,
    current_user: &CurrentUser,
    brand_id: BrandId,
    category_id: CategoryId,
) -> Result<(), AppError> {
    require_admin(current_user)?;

    // Verify both exist
    repository::get_brand_by_id(&state.pool, brand_id).await?;
    category_repo::get_category_by_id(&state.pool, category_id).await?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::associate_category(tx.as_executor(), brand_id, category_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to associate category: {}", e)))?;

    Ok(())
}

pub async fn disassociate_category(
    state: &AppState,
    current_user: &CurrentUser,
    brand_id: BrandId,
    category_id: CategoryId,
) -> Result<(), AppError> {
    require_admin(current_user)?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::disassociate_category(tx.as_executor(), brand_id, category_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| {
        AppError::InternalServerError(format!("Failed to disassociate category: {}", e))
    })?;

    Ok(())
}

pub async fn list_categories_for_brand(
    state: &AppState,
    brand_id: BrandId,
) -> Result<Vec<CategoryRes>, AppError> {
    repository::get_brand_by_id(&state.pool, brand_id).await?;
    let categories = repository::list_categories_for_brand(&state.pool, brand_id).await?;
    Ok(categories.into_iter().map(CategoryRes::new).collect())
}
