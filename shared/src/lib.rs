pub mod auth;
pub mod config;
pub mod db;
pub mod errors;

pub struct CommonAppState {
    pub current_user: Option<auth::jwt::CurrentUser>,
}

impl CommonAppState {
    pub fn new() -> Self {
        Self { current_user: None }
    }
}
