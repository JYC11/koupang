use crate::auth::jwt::CurrentUser;
use crate::errors::AppError;
use uuid::Uuid;

pub fn require_access(
    current_user: &CurrentUser,
    resource_owner_id: &Uuid,
) -> Result<(), AppError> {
    if current_user.can_access(resource_owner_id) {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "You don't have permission to access this resource".to_string(),
        ))
    }
}

pub fn require_admin(current_user: &CurrentUser) -> Result<(), AppError> {
    if current_user.role == "ADMIN" {
        Ok(())
    } else {
        Err(AppError::Forbidden("Admin access required".to_string()))
    }
}
