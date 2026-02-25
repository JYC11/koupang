use catalog::AppState;
use catalog::brands::service::BrandService;
use catalog::brands::value_objects::BrandId;
use catalog::categories::service::CategoryService;
use catalog::categories::value_objects::CategoryId;
use catalog::products::dtos::{AddProductImageReq, CreateProductReq, CreateSkuReq};
use catalog::products::service::CatalogService;
use rust_decimal::Decimal;
use shared::db::PgPool;
use shared::test_utils::auth::test_auth_config;
use shared::test_utils::db::TestDb;
use uuid::Uuid;

// Re-export shared auth helpers so tests can import from `crate::common::*`
pub use shared::test_utils::auth::{
    admin_user, admin_user as admin, seller_user, seller_user as seller, test_token,
};

pub async fn test_db() -> TestDb {
    TestDb::start("./migrations").await
}

pub fn test_catalog_service(pool: PgPool) -> CatalogService {
    CatalogService::new(pool)
}

pub fn test_category_service(pool: PgPool) -> CategoryService {
    CategoryService::new(pool)
}

pub fn test_brand_service(pool: PgPool) -> BrandService {
    BrandService::new(pool)
}

pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_jwt(pool, test_auth_config())
}

pub fn sample_create_product_req() -> CreateProductReq {
    CreateProductReq {
        name: "Test Widget".to_string(),
        slug: None, // auto-generated from name
        description: Some("A test product".to_string()),
        base_price: Decimal::new(1999, 2), // 19.99
        currency: None,                    // defaults to USD
        category_id: None,
        brand_id: None,
    }
}

pub fn sample_create_product_req_2() -> CreateProductReq {
    CreateProductReq {
        name: "Another Widget".to_string(),
        slug: Some("another-widget".to_string()),
        description: None,
        base_price: Decimal::new(4999, 2), // 49.99
        currency: Some("KRW".to_string()),
        category_id: None,
        brand_id: None,
    }
}

pub fn sample_create_sku_req() -> CreateSkuReq {
    CreateSkuReq {
        sku_code: "WIDGET-BLUE-XL".to_string(),
        price: Decimal::new(2499, 2), // 24.99
        stock_quantity: 100,
        attributes: Some(serde_json::json!({"color": "blue", "size": "XL"})),
    }
}

pub fn sample_add_image_req() -> AddProductImageReq {
    AddProductImageReq {
        url: "https://cdn.example.com/img/widget-1.jpg".to_string(),
        alt_text: Some("Widget front view".to_string()),
        sort_order: Some(0),
        is_primary: Some(true),
    }
}

// ── Category / Brand test fixtures ─────────────────────────

pub async fn create_test_category(pool: &PgPool) -> CategoryId {
    create_test_category_named(pool, "Electronics").await
}

pub async fn create_test_category_named(pool: &PgPool, name: &str) -> CategoryId {
    let slug = name.to_lowercase().replace(' ', "-");
    // ltree labels use underscores, not hyphens
    let path_label = slug.replace('-', "_");
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO categories (name, slug, path, depth) VALUES ($1, $2, $3::ltree, 0) RETURNING id",
    )
        .bind(name)
        .bind(&slug)
        .bind(&path_label)
        .fetch_one(pool)
        .await
        .expect("Failed to create test category");
    CategoryId::new(row.0)
}

pub async fn create_test_child_category(
    pool: &PgPool,
    parent_id: CategoryId,
    name: &str,
) -> CategoryId {
    let slug = name.to_lowercase().replace(' ', "-");
    let path_label = slug.replace('-', "_");

    let parent: (String, i32) =
        sqlx::query_as("SELECT path::text, depth FROM categories WHERE id = $1")
            .bind(parent_id.value())
            .fetch_one(pool)
            .await
            .expect("Failed to get parent category");

    let path = format!("{}.{}", parent.0, path_label);
    let depth = parent.1 + 1;

    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO categories (name, slug, path, parent_id, depth) VALUES ($1, $2, $3::ltree, $4, $5) RETURNING id",
    )
        .bind(name)
        .bind(&slug)
        .bind(&path)
        .bind(parent_id.value())
        .bind(depth)
        .fetch_one(pool)
        .await
        .expect("Failed to create child category");
    CategoryId::new(row.0)
}

pub async fn create_test_brand(pool: &PgPool) -> BrandId {
    create_test_brand_named(pool, "Acme Corp").await
}

pub async fn create_test_brand_named(pool: &PgPool, name: &str) -> BrandId {
    let slug = name.to_lowercase().replace(' ', "-");
    let row: (Uuid,) =
        sqlx::query_as("INSERT INTO brands (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(name)
            .bind(&slug)
            .fetch_one(pool)
            .await
            .expect("Failed to create test brand");
    BrandId::new(row.0)
}

pub async fn associate_brand_category(pool: &PgPool, brand_id: BrandId, category_id: CategoryId) {
    sqlx::query("INSERT INTO brand_categories (brand_id, category_id) VALUES ($1, $2)")
        .bind(brand_id.value())
        .bind(category_id.value())
        .execute(pool)
        .await
        .expect("Failed to associate brand with category");
}

pub fn sample_create_product_with_fks(
    category_id: Option<CategoryId>,
    brand_id: Option<BrandId>,
) -> CreateProductReq {
    CreateProductReq {
        name: "FK Test Widget".to_string(),
        slug: None,
        description: Some("A product with FK references".to_string()),
        base_price: Decimal::new(2999, 2),
        currency: None,
        category_id: category_id.map(|id| id.value()),
        brand_id: brand_id.map(|id| id.value()),
    }
}
