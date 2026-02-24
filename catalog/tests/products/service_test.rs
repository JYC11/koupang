use crate::common::{
    admin_user, associate_brand_category, create_test_brand, create_test_brand_named,
    create_test_category, sample_add_image_req, sample_create_product_req,
    sample_create_product_with_fks, sample_create_sku_req, seller_user, test_catalog_service,
    test_db,
};
use catalog::products::dtos::UpdateProductReq;
use catalog::products::value_objects::ProductStatus;

// ── Product service tests ───────────────────────────────────

#[tokio::test]
async fn create_and_get_product() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let product = service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();

    assert_eq!(product.name, "Test Widget");
    assert_eq!(product.slug, "test-widget");
    assert_eq!(product.seller_id, seller.id.to_string());

    // Get by ID
    let fetched = service
        .get_product(uuid::Uuid::parse_str(&product.id).unwrap())
        .await
        .unwrap();
    assert_eq!(fetched.name, "Test Widget");
}

#[tokio::test]
async fn get_product_by_slug() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();

    let product = service.get_product_by_slug("test-widget").await.unwrap();
    assert_eq!(product.name, "Test Widget");
}

#[tokio::test]
async fn get_product_detail_includes_skus_and_images() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let product = service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();

    // Add SKU and image
    service
        .create_sku(&seller, product_id, sample_create_sku_req())
        .await
        .unwrap();
    service
        .add_image(&seller, product_id, sample_add_image_req())
        .await
        .unwrap();

    let detail = service.get_product_detail(product_id).await.unwrap();
    assert_eq!(detail.skus.len(), 1);
    assert_eq!(detail.images.len(), 1);
}

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

    let active = service.list_active_products().await.unwrap();
    assert!(active.is_empty());

    // Activate the product
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();
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

    let active = service.list_active_products().await.unwrap();
    assert_eq!(active.len(), 1);
}

#[tokio::test]
async fn update_product_requires_ownership() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();
    let other_seller = seller_user(); // different UUID

    let product = service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();

    let result = service
        .update_product(
            &other_seller,
            product_id,
            UpdateProductReq {
                name: Some("Hacked".to_string()),
                slug: None,
                description: None,
                base_price: None,
                currency: None,
                category_id: None,
                brand_id: None,
                status: None,
            },
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn admin_can_update_any_product() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();
    let admin = admin_user();

    let product = service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();

    let result = service
        .update_product(
            &admin,
            product_id,
            UpdateProductReq {
                name: Some("Admin Updated".to_string()),
                slug: None,
                description: None,
                base_price: None,
                currency: None,
                category_id: None,
                brand_id: None,
                status: None,
            },
        )
        .await;

    assert!(result.is_ok());
}

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
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();

    let result = service.delete_product(&other_seller, product_id).await;
    assert!(result.is_err());

    // Owner can delete
    let result = service.delete_product(&seller, product_id).await;
    assert!(result.is_ok());
}

// ── SKU service tests ───────────────────────────────────────

#[tokio::test]
async fn create_and_list_skus() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let product = service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();

    let sku = service
        .create_sku(&seller, product_id, sample_create_sku_req())
        .await
        .unwrap();
    assert_eq!(sku.sku_code, "WIDGET-BLUE-XL");
    assert_eq!(sku.stock_quantity, 100);

    let skus = service.list_skus(product_id).await.unwrap();
    assert_eq!(skus.len(), 1);
}

#[tokio::test]
async fn adjust_stock() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let product = service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();

    let sku = service
        .create_sku(&seller, product_id, sample_create_sku_req())
        .await
        .unwrap();
    let sku_id = uuid::Uuid::parse_str(&sku.id).unwrap();

    service.adjust_stock(&seller, sku_id, -50).await.unwrap();

    let skus = service.list_skus(product_id).await.unwrap();
    assert_eq!(skus[0].stock_quantity, 50);
}

// ── Image service tests ─────────────────────────────────────

#[tokio::test]
async fn add_and_delete_image() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let product = service
        .create_product(&seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();

    let image = service
        .add_image(&seller, product_id, sample_add_image_req())
        .await
        .unwrap();
    assert!(image.is_primary);

    let images = service.list_images(product_id).await.unwrap();
    assert_eq!(images.len(), 1);

    let image_id = uuid::Uuid::parse_str(&image.id).unwrap();
    service
        .delete_image(&seller, product_id, image_id)
        .await
        .unwrap();

    let images = service.list_images(product_id).await.unwrap();
    assert!(images.is_empty());
}

// ── FK validation tests ────────────────────────────────────

#[tokio::test]
async fn create_product_with_valid_category_and_brand() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let cat_id = create_test_category(&db.pool).await;
    let brand_id = create_test_brand(&db.pool).await;
    associate_brand_category(&db.pool, brand_id, cat_id).await;

    let req = sample_create_product_with_fks(Some(cat_id), Some(brand_id));
    let product = service.create_product(&seller, req).await.unwrap();

    assert_eq!(product.category_name.as_deref(), Some("Electronics"));
    assert_eq!(product.category_slug.as_deref(), Some("electronics"));
    assert_eq!(product.brand_name.as_deref(), Some("Acme Corp"));
    assert_eq!(product.brand_slug.as_deref(), Some("acme-corp"));
}

#[tokio::test]
async fn create_product_with_nonexistent_category_fails() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let fake_id = uuid::Uuid::new_v4();
    let req = sample_create_product_with_fks(Some(fake_id), None);
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
    let req = sample_create_product_with_fks(None, Some(fake_id));
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
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();

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
                brand_id: Some(other_brand_id),
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

#[tokio::test]
async fn product_detail_includes_joined_names() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let seller = seller_user();

    let cat_id = create_test_category(&db.pool).await;
    let brand_id = create_test_brand(&db.pool).await;
    associate_brand_category(&db.pool, brand_id, cat_id).await;

    let req = sample_create_product_with_fks(Some(cat_id), Some(brand_id));
    let product = service.create_product(&seller, req).await.unwrap();
    let product_id = uuid::Uuid::parse_str(&product.id).unwrap();

    let detail = service.get_product_detail(product_id).await.unwrap();
    assert_eq!(detail.product.category_name.as_deref(), Some("Electronics"));
    assert_eq!(detail.product.brand_name.as_deref(), Some("Acme Corp"));
}
