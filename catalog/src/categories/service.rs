use crate::categories::dtos::{
    CategoryRes, CreateCategoryReq, UpdateCategoryReq, ValidCreateCategoryReq,
    ValidUpdateCategoryReq,
};
use crate::categories::repository;
use shared::auth::guards::require_admin;
use shared::auth::jwt::CurrentUser;
use shared::db::PgPool;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::errors::AppError;
use uuid::Uuid;

pub struct CategoryService {
    pool: PgPool,
}

impl CategoryService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_category(
        &self,
        current_user: &CurrentUser,
        req: CreateCategoryReq,
    ) -> Result<CategoryRes, AppError> {
        require_admin(current_user)?;

        let validated: ValidCreateCategoryReq = req.try_into()?;

        // Compute path and depth from parent
        let (path, depth) = match validated.parent_id {
            Some(parent_id) => {
                let parent = repository::get_category_by_id(&self.pool, parent_id).await?;
                let path = format!("{}.{}", parent.path, validated.label.as_str());
                let depth = parent.depth + 1;
                (path, depth)
            }
            None => (validated.label.as_str().to_string(), 0),
        };

        let category_id = with_transaction(&self.pool, |tx| {
            let path = path.clone();
            Box::pin(async move {
                repository::create_category(tx.as_executor(), validated, &path, depth)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to create category: {}", e)))?;

        let category = repository::get_category_by_id(&self.pool, category_id).await?;
        Ok(CategoryRes::new(category))
    }

    pub async fn get_category(&self, id: Uuid) -> Result<CategoryRes, AppError> {
        let category = repository::get_category_by_id(&self.pool, id).await?;
        Ok(CategoryRes::new(category))
    }

    pub async fn get_category_by_slug(&self, slug: &str) -> Result<CategoryRes, AppError> {
        let category = repository::get_category_by_slug(&self.pool, slug).await?;
        Ok(CategoryRes::new(category))
    }

    pub async fn list_root_categories(&self) -> Result<Vec<CategoryRes>, AppError> {
        let categories = repository::list_root_categories(&self.pool).await?;
        Ok(categories.into_iter().map(CategoryRes::new).collect())
    }

    pub async fn get_children(&self, id: Uuid) -> Result<Vec<CategoryRes>, AppError> {
        // Verify parent exists
        repository::get_category_by_id(&self.pool, id).await?;
        let children = repository::get_children(&self.pool, id).await?;
        Ok(children.into_iter().map(CategoryRes::new).collect())
    }

    pub async fn get_subtree(&self, id: Uuid) -> Result<Vec<CategoryRes>, AppError> {
        let category = repository::get_category_by_id(&self.pool, id).await?;
        let subtree = repository::get_subtree(&self.pool, &category.path).await?;
        Ok(subtree.into_iter().map(CategoryRes::new).collect())
    }

    pub async fn get_ancestors(&self, id: Uuid) -> Result<Vec<CategoryRes>, AppError> {
        let category = repository::get_category_by_id(&self.pool, id).await?;
        let ancestors = repository::get_ancestors(&self.pool, &category.path).await?;
        Ok(ancestors.into_iter().map(CategoryRes::new).collect())
    }

    pub async fn update_category(
        &self,
        current_user: &CurrentUser,
        id: Uuid,
        req: UpdateCategoryReq,
    ) -> Result<(), AppError> {
        require_admin(current_user)?;

        // Verify exists
        repository::get_category_by_id(&self.pool, id).await?;

        let validated: ValidUpdateCategoryReq = req.try_into()?;

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::update_category(tx.as_executor(), id, validated)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update category: {}", e)))?;

        Ok(())
    }

    pub async fn delete_category(
        &self,
        current_user: &CurrentUser,
        id: Uuid,
    ) -> Result<(), AppError> {
        require_admin(current_user)?;

        // Verify exists
        repository::get_category_by_id(&self.pool, id).await?;

        // Guard: cannot delete if has children
        if repository::has_children(&self.pool, id).await? {
            return Err(AppError::BadRequest(
                "Cannot delete category with child categories. Delete children first.".to_string(),
            ));
        }

        // Guard: cannot delete if products reference it
        if repository::has_products(&self.pool, id).await? {
            return Err(AppError::BadRequest(
                "Cannot delete category with associated products. Reassign products first."
                    .to_string(),
            ));
        }

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                repository::delete_category(tx.as_executor(), id)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete category: {}", e)))?;

        Ok(())
    }
}
