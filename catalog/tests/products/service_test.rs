use crate::common::{
    admin_user, sample_add_image_req, sample_create_product_req, sample_create_sku_req,
    seller_user, test_catalog_service, test_db,
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
                category: None,
                brand: None,
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
                category: None,
                brand: None,
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
                category: None,
                brand: None,
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
