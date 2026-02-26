use crate::common::{
    associate_brand_category, create_test_brand, create_test_brand_named, create_test_category,
    sample_create_product_req, sample_create_product_with_fks, seller_user, test_catalog_service,
    test_db,
};
use catalog::products::dtos::{ProductFilter, UpdateProductReq};
use catalog::products::value_objects::{ProductId, ProductStatus};
use shared::db::pagination_support::PaginationParams;

fn default_params() -> PaginationParams {
    PaginationParams::default()
}

fn default_filter() -> ProductFilter {
    ProductFilter {
        category_id: None,
        brand_id: None,
        min_price: None,
        max_price: None,
        search: None,
        status: None,
    }
}

// ── Business logic tests ────────────────────────────────────

#[tokio::test]
async fn list_active_products_excludes_drafts() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    // Product starts as draft
    let product = service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();

    let result = service
        .list_active_products(default_params(), default_filter())
        .await
        .unwrap();
    assert!(result.items.is_empty());

    // Activate the product
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());
    service
        .update_product(
            &seller,
            product_id,
            UpdateProductReq {
                name: None,
                slug: None,
                description: None,
                base_price: None,
                currency: None,
                category_id: None,
                brand_id: None,
                status: Some(ProductStatus::Active),
            },
        )
        .await
        .unwrap();

    let result = service
        .list_active_products(default_params(), default_filter())
        .await
        .unwrap();
    assert_eq!(result.items.len(), 1);
}

// ── Ownership guard tests ───────────────────────────────────

#[tokio::test]
async fn delete_product_requires_ownership() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();
    let other_seller = seller_user();

    let product = service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());

    let result = service.delete_product(&other_seller, product_id).await;
    assert!(result.is_err());

    // Owner can delete
    let result = service.delete_product(&seller, product_id).await;
    assert!(result.is_ok());
}

// ── FK validation tests ─────────────────────────────────────

#[tokio::test]
async fn create_product_with_nonexistent_category_fails() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let fake_id = uuid::Uuid::new_v4();
    let req = sample_create_product_with_fks(
        Some(catalog::categories::value_objects::CategoryId::new(fake_id)),
        None,
    );
    let result = service.create_product(&seller, req).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Category does not exist"), "got: {}", err);
}

#[tokio::test]
async fn create_product_with_nonexistent_brand_fails() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let fake_id = uuid::Uuid::new_v4();
    let req = sample_create_product_with_fks(
        None,
        Some(catalog::brands::value_objects::BrandId::new(fake_id)),
    );
    let result = service.create_product(&seller, req).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Brand does not exist"), "got: {}", err);
}

#[tokio::test]
async fn create_product_with_brand_not_in_category_fails() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let cat_id = create_test_category(&db.pool).await;
    let brand_id = create_test_brand(&db.pool).await;
    // Not associating brand with category

    let req = sample_create_product_with_fks(Some(cat_id), Some(brand_id));
    let result = service.create_product(&seller, req).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Brand is not associated with the specified category"),
        "got: {}",
        err
    );
}

#[tokio::test]
async fn create_product_with_only_category_succeeds() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let cat_id = create_test_category(&db.pool).await;

    let req = sample_create_product_with_fks(Some(cat_id), None);
    let product = service.create_product(&seller, req).await.unwrap();

    assert_eq!(product.category_name.as_deref(), Some("Electronics"));
    assert!(product.brand_name.is_none());
}

#[tokio::test]
async fn create_product_with_only_brand_succeeds() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let brand_id = create_test_brand(&db.pool).await;

    let req = sample_create_product_with_fks(None, Some(brand_id));
    let product = service.create_product(&seller, req).await.unwrap();

    assert!(product.category_name.is_none());
    assert_eq!(product.brand_name.as_deref(), Some("Acme Corp"));
}

#[tokio::test]
async fn update_product_brand_not_in_existing_category_fails() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let cat_id = create_test_category(&db.pool).await;
    let brand_id = create_test_brand(&db.pool).await;
    associate_brand_category(&db.pool, brand_id, cat_id).await;

    // Create product with valid category + brand
    let req = sample_create_product_with_fks(Some(cat_id), Some(brand_id));
    let product = service.create_product(&seller, req).await.unwrap();
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());

    // Create a second brand NOT associated with the category
    let other_brand_id = create_test_brand_named(&db.pool, "Other Brand").await;

    // Try updating to the unassociated brand
    let result = service
        .update_product(
            &seller,
            product_id,
            UpdateProductReq {
                name: None,
                slug: None,
                description: None,
                base_price: None,
                currency: None,
                category_id: None,
                brand_id: Some(other_brand_id.value()),
                status: None,
            },
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Brand is not associated with the specified category"),
        "got: {}",
        err
    );
}
