use crate::users::entities::UserEntity;
use crate::users::value_objects::{Email, Password, Phone, Username};
use shared::auth::Role;
use shared::errors::AppError;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub username: Username,
    pub password: Password,
    pub email: Email,
    pub phone: Phone,
    pub role: Role,
}

impl TryFrom<UserEntity> for User {
    type Error = AppError;

    fn try_from(entity: UserEntity) -> Result<Self, Self::Error> {
        Ok(Self {
            id: entity.id,
            username: Username::new(&entity.username)?,
            password: Password::new(&entity.password)?,
            email: Email::new(&entity.email)?,
            phone: Phone::new(&entity.phone)?,
            role: entity.role,
        })
    }
}
