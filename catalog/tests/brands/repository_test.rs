use crate::common::{create_test_category, test_db};
use catalog::brands::dtos::{ValidCreateBrandReq, ValidUpdateBrandReq};
use catalog::brands::repository;
use catalog::brands::value_objects::{BrandId, BrandName};
use catalog::common::value_objects::{HttpUrl, Slug};
use uuid::Uuid;

fn sample_create_brand(name: &str) -> ValidCreateBrandReq {
    ValidCreateBrandReq {
        name: BrandName::new(name).unwrap(),
        slug: Slug::from_name(name).unwrap(),
        description: Some(format!("{} description", name)),
        logo_url: Some(HttpUrl::new("https://example.com/logo.png").unwrap()),
    }
}

// ── CRUD tests ──────────────────────────────────────────────

#[tokio::test]
async fn create_and_get_brand() {
    let db = test_db().await;
    let req = sample_create_brand("Acme Corp");

    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, req).await.unwrap();

    let brand = repository::get_brand_by_id(&db.pool, brand_id)
        .await
        .unwrap();

    assert_eq!(brand.id, brand_id.value());
    assert_eq!(brand.name, "Acme Corp");
    assert_eq!(brand.slug, "acme-corp");
    assert_eq!(brand.description.as_deref(), Some("Acme Corp description"));
    assert_eq!(
        brand.logo_url.as_deref(),
        Some("https://example.com/logo.png")
    );
}

#[tokio::test]
async fn get_brand_by_slug() {
    let db = test_db().await;
    let req = sample_create_brand("Samsung");

    let mut conn = db.pool.acquire().await.unwrap();
    repository::create_brand(&mut *conn, req).await.unwrap();

    let brand = repository::get_brand_by_slug(&db.pool, "samsung")
        .await
        .unwrap();

    assert_eq!(brand.name, "Samsung");
}

#[tokio::test]
async fn get_nonexistent_brand_returns_error() {
    let db = test_db().await;
    let result = repository::get_brand_by_id(&db.pool, BrandId::new(Uuid::new_v4())).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_brand_by_nonexistent_slug_returns_error() {
    let db = test_db().await;
    let result = repository::get_brand_by_slug(&db.pool, "no-such-brand").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn list_brands() {
    let db = test_db().await;

    let mut conn = db.pool.acquire().await.unwrap();
    repository::create_brand(&mut *conn, sample_create_brand("Beta Brand"))
        .await
        .unwrap();
    repository::create_brand(&mut *conn, sample_create_brand("Alpha Brand"))
        .await
        .unwrap();

    let brands = repository::list_brands(&db.pool).await.unwrap();

    assert_eq!(brands.len(), 2);
    // Alphabetical order
    assert_eq!(brands[0].name, "Alpha Brand");
    assert_eq!(brands[1].name, "Beta Brand");
}

#[tokio::test]
async fn list_brands_empty() {
    let db = test_db().await;
    let brands = repository::list_brands(&db.pool).await.unwrap();
    assert!(brands.is_empty());
}

// ── Update tests ────────────────────────────────────────────

#[tokio::test]
async fn update_brand_name() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, sample_create_brand("Old Name"))
        .await
        .unwrap();

    let update = ValidUpdateBrandReq {
        name: Some(BrandName::new("New Name").unwrap()),
        description: None,
        logo_url: None,
    };
    repository::update_brand(&mut *conn, brand_id, update)
        .await
        .unwrap();

    let brand = repository::get_brand_by_id(&db.pool, brand_id)
        .await
        .unwrap();
    assert_eq!(brand.name, "New Name");
    assert!(brand.updated_at.is_some());
}

#[tokio::test]
async fn update_brand_description() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, sample_create_brand("Test Brand"))
        .await
        .unwrap();

    let update = ValidUpdateBrandReq {
        name: None,
        description: Some("Updated description".to_string()),
        logo_url: None,
    };
    repository::update_brand(&mut *conn, brand_id, update)
        .await
        .unwrap();

    let brand = repository::get_brand_by_id(&db.pool, brand_id)
        .await
        .unwrap();
    assert_eq!(brand.description.as_deref(), Some("Updated description"));
}

#[tokio::test]
async fn update_brand_logo_url() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, sample_create_brand("Test Brand"))
        .await
        .unwrap();

    let update = ValidUpdateBrandReq {
        name: None,
        description: None,
        logo_url: Some(HttpUrl::new("https://cdn.example.com/new-logo.png").unwrap()),
    };
    repository::update_brand(&mut *conn, brand_id, update)
        .await
        .unwrap();

    let brand = repository::get_brand_by_id(&db.pool, brand_id)
        .await
        .unwrap();
    assert_eq!(
        brand.logo_url.as_deref(),
        Some("https://cdn.example.com/new-logo.png")
    );
}

#[tokio::test]
async fn update_brand_all_fields() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, sample_create_brand("Original"))
        .await
        .unwrap();

    let update = ValidUpdateBrandReq {
        name: Some(BrandName::new("Updated").unwrap()),
        description: Some("All new".to_string()),
        logo_url: Some(HttpUrl::new("https://cdn.example.com/updated.png").unwrap()),
    };
    repository::update_brand(&mut *conn, brand_id, update)
        .await
        .unwrap();

    let brand = repository::get_brand_by_id(&db.pool, brand_id)
        .await
        .unwrap();
    assert_eq!(brand.name, "Updated");
    assert_eq!(brand.description.as_deref(), Some("All new"));
    assert_eq!(
        brand.logo_url.as_deref(),
        Some("https://cdn.example.com/updated.png")
    );
}

#[tokio::test]
async fn update_nonexistent_brand_returns_error() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();

    let update = ValidUpdateBrandReq {
        name: Some(BrandName::new("Ghost").unwrap()),
        description: None,
        logo_url: None,
    };
    let result = repository::update_brand(&mut *conn, BrandId::new(Uuid::new_v4()), update).await;
    assert!(result.is_err());
}

// ── Delete tests ────────────────────────────────────────────

#[tokio::test]
async fn delete_brand() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, sample_create_brand("Doomed Brand"))
        .await
        .unwrap();

    repository::delete_brand(&mut *conn, brand_id)
        .await
        .unwrap();

    let result = repository::get_brand_by_id(&db.pool, brand_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn delete_nonexistent_brand_returns_error() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let result = repository::delete_brand(&mut *conn, BrandId::new(Uuid::new_v4())).await;
    assert!(result.is_err());
}

// ── has_products ────────────────────────────────────────────

#[tokio::test]
async fn has_products_false_when_none() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, sample_create_brand("Empty Brand"))
        .await
        .unwrap();

    let result = repository::has_products(&db.pool, brand_id).await.unwrap();
    assert!(!result);
}

// ── Brand-Category association tests ────────────────────────

#[tokio::test]
async fn associate_and_list_categories() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, sample_create_brand("Multi Brand"))
        .await
        .unwrap();

    let cat_id = create_test_category(&db.pool).await;

    repository::associate_category(&mut *conn, brand_id, cat_id)
        .await
        .unwrap();

    let categories = repository::list_categories_for_brand(&db.pool, brand_id)
        .await
        .unwrap();

    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].id, cat_id.value());
}

#[tokio::test]
async fn disassociate_category() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, sample_create_brand("Temp Brand"))
        .await
        .unwrap();

    let cat_id = create_test_category(&db.pool).await;

    repository::associate_category(&mut *conn, brand_id, cat_id)
        .await
        .unwrap();
    repository::disassociate_category(&mut *conn, brand_id, cat_id)
        .await
        .unwrap();

    let categories = repository::list_categories_for_brand(&db.pool, brand_id)
        .await
        .unwrap();
    assert!(categories.is_empty());
}

#[tokio::test]
async fn disassociate_nonexistent_returns_error() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let brand_id = repository::create_brand(&mut *conn, sample_create_brand("Lonely Brand"))
        .await
        .unwrap();

    let cat_id = create_test_category(&db.pool).await;

    let result = repository::disassociate_category(&mut *conn, brand_id, cat_id).await;
    assert!(result.is_err());
}
