use crate::common::{create_test_category, test_db};
use catalog::brands::dtos::ValidCreateBrandReq;
use catalog::brands::repository;
use catalog::brands::value_objects::BrandName;
use catalog::common::value_objects::{HttpUrl, Slug};

fn sample_create_brand(name: &str) -> ValidCreateBrandReq {
    ValidCreateBrandReq {
        name: BrandName::new(name).unwrap(),
        slug: Slug::from_name(name).unwrap(),
        description: Some(format!("{} description", name)),
        logo_url: Some(HttpUrl::new("https://example.com/logo.png").unwrap()),
    }
}

// ── Internal helper tests ───────────────────────────────────

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

// ── Error path tests ────────────────────────────────────────

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
