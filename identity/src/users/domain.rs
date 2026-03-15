use crate::users::entities::UserEntity;
use crate::users::value_objects::{Email, HashedPassword, Phone, UserId, Username};
use shared::auth::Role;
use shared::errors::AppError;

#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub username: Username,
    pub password: HashedPassword,
    pub email: Email,
    pub phone: Phone,
    pub role: Role,
}

impl TryFrom<UserEntity> for User {
    type Error = AppError;

    fn try_from(entity: UserEntity) -> Result<Self, Self::Error> {
        Ok(Self {
            id: UserId::new(entity.id),
            username: Username::new(&entity.username)?,
            password: HashedPassword::new(entity.password),
            email: Email::new(&entity.email)?,
            phone: Phone::new(&entity.phone)?,
            role: entity.role,
        })
    }
}
