use crate::common::{sample_create_product_req, sample_create_sku_req, sample_add_image_req, test_db};
use catalog::products::dtos::{
    ValidAddProductImageReq, ValidCreateProductReq, ValidCreateSkuReq,
};
use catalog::products::repository;
use uuid::Uuid;

fn validated_product(req: catalog::products::dtos::CreateProductReq) -> ValidCreateProductReq {
    req.try_into().expect("sample data should be valid")
}

fn validated_sku(req: catalog::products::dtos::CreateSkuReq) -> ValidCreateSkuReq {
    req.try_into().expect("sample data should be valid")
}

fn validated_image(req: catalog::products::dtos::AddProductImageReq) -> ValidAddProductImageReq {
    req.try_into().expect("sample data should be valid")
}

// ── Product tests ───────────────────────────────────────────

#[tokio::test]
async fn create_and_get_product() {
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

    assert_eq!(product.seller_id, seller_id);
    assert_eq!(product.name, "Test Widget");
    assert_eq!(product.slug, "test-widget");
    assert_eq!(product.currency, "USD");
}

#[tokio::test]
async fn get_product_by_slug() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let validated = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    repository::create_product(&mut *conn, seller_id, validated)
        .await
        .unwrap();

    let product = repository::get_product_by_slug(&db.pool, "test-widget")
        .await
        .unwrap();

    assert_eq!(product.name, "Test Widget");
}

#[tokio::test]
async fn get_nonexistent_product_returns_error() {
    let db = test_db().await;
    let result = repository::get_product_by_id(&db.pool, Uuid::new_v4()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn list_products_by_seller() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();

    let mut conn = db.pool.acquire().await.unwrap();
    let v1 = validated_product(sample_create_product_req());
    repository::create_product(&mut *conn, seller_id, v1)
        .await
        .unwrap();

    let mut req2 = crate::common::sample_create_product_req_2();
    req2.slug = Some("second-product".to_string());
    let v2 = validated_product(req2);
    repository::create_product(&mut *conn, seller_id, v2)
        .await
        .unwrap();

    let products = repository::list_products_by_seller(&db.pool, seller_id)
        .await
        .unwrap();

    assert_eq!(products.len(), 2);
}

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

// ── SKU tests ───────────────────────────────────────────────

#[tokio::test]
async fn create_and_list_skus() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let validated_product = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, validated_product)
        .await
        .unwrap();

    let validated_sku = validated_sku(sample_create_sku_req());
    let sku_id = repository::create_sku(&mut *conn, product_id, validated_sku)
        .await
        .unwrap();

    let sku = repository::get_sku_by_id(&db.pool, sku_id).await.unwrap();
    assert_eq!(sku.sku_code, "WIDGET-BLUE-XL");
    assert_eq!(sku.stock_quantity, 100);

    let skus = repository::list_skus_by_product(&db.pool, product_id)
        .await
        .unwrap();
    assert_eq!(skus.len(), 1);
}

#[tokio::test]
async fn adjust_stock_increases_and_decreases() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let validated_product = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, validated_product)
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
    let validated_product = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, validated_product)
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

// ── Image tests ─────────────────────────────────────────────

#[tokio::test]
async fn add_and_list_images() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let validated_product = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, validated_product)
        .await
        .unwrap();

    let validated_image = validated_image(sample_add_image_req());
    repository::add_product_image(&mut *conn, product_id, validated_image)
        .await
        .unwrap();

    let images = repository::list_images_by_product(&db.pool, product_id)
        .await
        .unwrap();
    assert_eq!(images.len(), 1);
    assert!(images[0].is_primary);
    assert_eq!(images[0].url, "https://cdn.example.com/img/widget-1.jpg");
}

#[tokio::test]
async fn delete_image() {
    let db = test_db().await;
    let seller_id = Uuid::new_v4();
    let validated_product = validated_product(sample_create_product_req());

    let mut conn = db.pool.acquire().await.unwrap();
    let product_id = repository::create_product(&mut *conn, seller_id, validated_product)
        .await
        .unwrap();

    let validated_image = validated_image(sample_add_image_req());
    let image_id = repository::add_product_image(&mut *conn, product_id, validated_image)
        .await
        .unwrap();

    repository::delete_product_image(&mut *conn, image_id)
        .await
        .unwrap();

    let images = repository::list_images_by_product(&db.pool, product_id)
        .await
        .unwrap();
    assert!(images.is_empty());
}
