use crate::common::{
    associate_brand_category, create_test_brand, create_test_category, sample_create_product_req,
    sample_create_product_with_fks, sample_create_sku_req, test_db,
};
use catalog::brands::value_objects::BrandId;
use catalog::categories::value_objects::CategoryId;
use catalog::products::dtos::{ValidCreateProductReq, ValidCreateSkuReq};
use catalog::products::repository;
use catalog::products::value_objects::{Currency, Price, ProductName, Slug};
use uuid::Uuid;

/// VO-validate a CreateProductReq into a ValidatedCreateProduct, bypassing FK checks for repo tests.
fn validated_product(req: catalog::products::dtos::CreateProductReq) -> ValidCreateProductReq {
    let name = ProductName::new(&req.name).expect("sample name should be valid");
    let slug = match req.slug {
        Some(s) => Slug::new(&s).expect("sample slug should be valid"),
        None => Slug::from_name(name.as_str()).expect("slug from name should be valid"),
    };
    let base_price = Price::new(req.base_price).expect("sample price should be valid");
    let currency = match req.currency {
        Some(c) => Currency::new(&c).expect("sample currency should be valid"),
        None => Currency::default(),
    };
    ValidCreateProductReq {
        name,
        slug,
        description: req.description,
        base_price,
        currency,
        category_id: req.category_id.map(CategoryId::new),
        brand_id: req.brand_id.map(BrandId::new),
    }
}

fn validated_sku(req: catalog::products::dtos::CreateSkuReq) -> ValidCreateSkuReq {
    req.try_into().expect("sample data should be valid")
}

// ── Soft delete behavior ────────────────────────────────────

#[tokio::test]
async fn delete_product_soft_deletes() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let validated = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, validated)
        .await
        .unwrap();

    repository::delete_product(&mut *conn, product_id)
        .await
        .unwrap();

    // Should not be found (soft deleted)
    let result = repository::get_product_by_id(&db.pool, product_id).await;
    assert!(result.is_err());
}

// ── Stock adjustment SQL correctness ────────────────────────

#[tokio::test]
async fn adjust_stock_increases_and_decreases() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let vp = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, vp)
        .await
        .unwrap();

    let validated_sku = validated_sku(sample_create_sku_req());
    let sku_id = repository::create_sku(&mut *conn, product_id, validated_sku)
        .await
        .unwrap();

    // Increase stock by 50
    repository::adjust_stock(&mut *conn, sku_id, 50)
        .await
        .unwrap();
    let sku = repository::get_sku_by_id(&db.pool, sku_id).await.unwrap();
    assert_eq!(sku.stock_quantity, 150);

    // Decrease stock by 30
    repository::adjust_stock(&mut *conn, sku_id, -30)
        .await
        .unwrap();
    let sku = repository::get_sku_by_id(&db.pool, sku_id).await.unwrap();
    assert_eq!(sku.stock_quantity, 120);
}

#[tokio::test]
async fn adjust_stock_rejects_going_negative() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let vp = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, vp)
        .await
        .unwrap();

    let validated_sku = validated_sku(sample_create_sku_req());
    let sku_id = repository::create_sku(&mut *conn, product_id, validated_sku)
        .await
        .unwrap();

    // Try to decrease by more than available (100)
    let result = repository::adjust_stock(&mut *conn, sku_id, -101).await;
    assert!(result.is_err());
}

// ── FK validation helper tests ──────────────────────────────

#[tokio::test]
async fn category_exists_returns_true_for_existing() {
    let db = test_db().await;
    let cat_id = create_test_category(&db.pool).await;

    assert!(repository::category_exists(&db.pool, cat_id).await.unwrap());
}

#[tokio::test]
async fn category_exists_returns_false_for_nonexistent() {
    let db = test_db().await;
    assert!(
        !repository::category_exists(&db.pool, CategoryId::new(Uuid::new_v4()))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn brand_exists_returns_true_for_existing() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;

    assert!(repository::brand_exists(&db.pool, brand_id).await.unwrap());
}

#[tokio::test]
async fn brand_exists_returns_false_for_nonexistent() {
    let db = test_db().await;
    assert!(
        !repository::brand_exists(&db.pool, BrandId::new(Uuid::new_v4()))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn is_brand_in_category_true_when_associated() {
    let db = test_db().await;
    let cat_id = create_test_category(&db.pool).await;
    let brand_id = create_test_brand(&db.pool).await;
    associate_brand_category(&db.pool, brand_id, cat_id).await;

    assert!(
        repository::is_brand_in_category(&db.pool, brand_id, cat_id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn is_brand_in_category_false_when_not_associated() {
    let db = test_db().await;
    let cat_id = create_test_category(&db.pool).await;
    let brand_id = create_test_brand(&db.pool).await;

    assert!(
        !repository::is_brand_in_category(&db.pool, brand_id, cat_id)
            .await
            .unwrap()
    );
}

// ── JOIN behavior tests ─────────────────────────────────────

#[tokio::test]
async fn get_product_by_id_includes_joined_fields() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let cat_id = create_test_category(&db.pool).await;
    let brand_id = create_test_brand(&db.pool).await;
    associate_brand_category(&db.pool, brand_id, cat_id).await;

    let dp = validated_product(sample_create_product_with_fks(Some(cat_id), Some(brand_id)));

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, dp)
        .await
        .unwrap();

    let product = repository::get_product_by_id(&db.pool, product_id)
        .await
        .unwrap();

    assert_eq!(product.category_name.as_deref(), Some("Electronics"));
    assert_eq!(product.category_slug.as_deref(), Some("electronics"));
    assert_eq!(product.brand_name.as_deref(), Some("Acme Corp"));
    assert_eq!(product.brand_slug.as_deref(), Some("acme-corp"));
}

#[tokio::test]
async fn get_product_by_id_returns_none_fields_when_no_fks() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let validated = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, validated)
        .await
        .unwrap();

    let product = repository::get_product_by_id(&db.pool, product_id)
        .await
        .unwrap();

    assert!(product.category_name.is_none());
    assert!(product.category_slug.is_none());
    assert!(product.brand_name.is_none());
    assert!(product.brand_slug.is_none());
}
