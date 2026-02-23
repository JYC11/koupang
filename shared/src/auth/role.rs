use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "UPPERCASE")]
#[sqlx(type_name = "varchar", rename_all = "UPPERCASE")]
pub enum Role {
    Buyer,
    Seller,
    Admin,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Buyer => write!(f, "BUYER"),
            Role::Seller => write!(f, "SELLER"),
            Role::Admin => write!(f, "ADMIN"),
        }
    }
}
