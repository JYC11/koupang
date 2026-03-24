use catalog::AppState;
use shared::db::PgPool;
use shared::test_utils::auth::test_auth_config;
use shared::test_utils::db::TestDb;

// Re-export shared auth helpers so tests can import from `crate::common::*`.
pub use shared::test_utils::auth::{
    admin_user, admin_user as admin, seller_user, seller_user as seller, test_token,
};

// Re-export test fixtures so existing tests keep working via `crate::common::*`.
pub use catalog::test_fixtures::{
    associate_brand_category, create_test_brand, create_test_brand_named, create_test_category,
    create_test_category_named, create_test_child_category, sample_add_image_req,
    sample_create_product_req, sample_create_product_req_2, sample_create_product_with_fks,
    sample_create_sku_req,
};

pub async fn test_db() -> TestDb {
    TestDb::start("./migrations").await
}

pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_jwt(pool, test_auth_config())
}
