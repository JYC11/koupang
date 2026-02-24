use crate::common::{
    admin_user, associate_brand_category, create_test_brand, create_test_brand_named,
    create_test_category, create_test_category_named, sample_create_product_with_fks, seller_user,
    test_brand_service, test_catalog_service, test_db,
};
use catalog::brands::dtos::{CreateBrandReq, UpdateBrandReq};

// ── Create ──────────────────────────────────────────────────

#[tokio::test]
async fn create_and_get_brand() {
    let db = test_db().await;
    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    let req = CreateBrandReq {
        name: "Samsung".to_string(),
        slug: None,
        description: Some("Korean electronics".to_string()),
        logo_url: Some("https://cdn.example.com/samsung.png".to_string()),
    };

    let brand = service.create_brand(&admin, req).await.unwrap();
    assert_eq!(brand.name, "Samsung");
    assert_eq!(brand.slug, "samsung");
    assert_eq!(brand.description.as_deref(), Some("Korean electronics"));
    assert_eq!(
        brand.logo_url.as_deref(),
        Some("https://cdn.example.com/samsung.png")
    );

    // Get by ID
    let id = uuid::Uuid::parse_str(&brand.id).unwrap();
    let fetched = service.get_brand(id).await.unwrap();
    assert_eq!(fetched.name, "Samsung");
}

#[tokio::test]
async fn get_brand_by_slug() {
    let db = test_db().await;
    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    service
        .create_brand(
            &admin,
            CreateBrandReq {
                name: "LG Electronics".to_string(),
                slug: None,
                description: None,
                logo_url: None,
            },
        )
        .await
        .unwrap();

    let fetched = service.get_brand_by_slug("lg-electronics").await.unwrap();
    assert_eq!(fetched.name, "LG Electronics");
}

#[tokio::test]
async fn get_nonexistent_brand_fails() {
    let db = test_db().await;
    let service = test_brand_service(db.pool.clone());

    let result = service.get_brand(uuid::Uuid::new_v4()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn list_brands() {
    let db = test_db().await;
    create_test_brand_named(&db.pool, "Samsung").await;
    create_test_brand_named(&db.pool, "Apple").await;

    let service = test_brand_service(db.pool.clone());
    let brands = service.list_brands().await.unwrap();
    assert_eq!(brands.len(), 2);

    // Ordered by name ASC
    assert_eq!(brands[0].name, "Apple");
    assert_eq!(brands[1].name, "Samsung");
}

#[tokio::test]
async fn create_brand_requires_admin() {
    let db = test_db().await;
    let service = test_brand_service(db.pool.clone());
    let seller = seller_user();

    let req = CreateBrandReq {
        name: "Hacked".to_string(),
        slug: None,
        description: None,
        logo_url: None,
    };

    let result = service.create_brand(&seller, req).await;
    assert!(result.is_err());
}

// ── Update ──────────────────────────────────────────────────

#[tokio::test]
async fn update_brand_name() {
    let db = test_db().await;
    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    let brand = service
        .create_brand(
            &admin,
            CreateBrandReq {
                name: "Old Brand".to_string(),
                slug: None,
                description: None,
                logo_url: None,
            },
        )
        .await
        .unwrap();

    let id = uuid::Uuid::parse_str(&brand.id).unwrap();
    service
        .update_brand(
            &admin,
            id,
            UpdateBrandReq {
                name: Some("New Brand".to_string()),
                description: None,
                logo_url: None,
            },
        )
        .await
        .unwrap();

    let updated = service.get_brand(id).await.unwrap();
    assert_eq!(updated.name, "New Brand");
}

#[tokio::test]
async fn update_brand_logo_url() {
    let db = test_db().await;
    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    let brand = service
        .create_brand(
            &admin,
            CreateBrandReq {
                name: "Brandish".to_string(),
                slug: None,
                description: None,
                logo_url: None,
            },
        )
        .await
        .unwrap();

    let id = uuid::Uuid::parse_str(&brand.id).unwrap();
    service
        .update_brand(
            &admin,
            id,
            UpdateBrandReq {
                name: None,
                description: None,
                logo_url: Some("https://cdn.example.com/new-logo.png".to_string()),
            },
        )
        .await
        .unwrap();

    let updated = service.get_brand(id).await.unwrap();
    assert_eq!(
        updated.logo_url.as_deref(),
        Some("https://cdn.example.com/new-logo.png")
    );
}

#[tokio::test]
async fn update_brand_requires_admin() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;
    let service = test_brand_service(db.pool.clone());
    let seller = seller_user();

    let result = service
        .update_brand(
            &seller,
            brand_id,
            UpdateBrandReq {
                name: Some("Hacked".to_string()),
                description: None,
                logo_url: None,
            },
        )
        .await;
    assert!(result.is_err());
}

// ── Delete ──────────────────────────────────────────────────

#[tokio::test]
async fn delete_brand() {
    let db = test_db().await;
    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    let brand = service
        .create_brand(
            &admin,
            CreateBrandReq {
                name: "Disposable".to_string(),
                slug: None,
                description: None,
                logo_url: None,
            },
        )
        .await
        .unwrap();

    let id = uuid::Uuid::parse_str(&brand.id).unwrap();
    service.delete_brand(&admin, id).await.unwrap();

    let result = service.get_brand(id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn delete_brand_with_products_fails() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;

    // Create a product referencing this brand
    let catalog = test_catalog_service(db.pool.clone());
    let seller = seller_user();
    catalog
        .create_product(&seller, sample_create_product_with_fks(None, Some(brand_id)))
        .await
        .unwrap();

    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    let result = service.delete_brand(&admin, brand_id).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("associated products"),
        "Expected products guard, got: {}",
        err
    );
}

#[tokio::test]
async fn delete_brand_requires_admin() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;
    let service = test_brand_service(db.pool.clone());
    let seller = seller_user();

    let result = service.delete_brand(&seller, brand_id).await;
    assert!(result.is_err());
}

// ── Brand-Category associations ─────────────────────────────

#[tokio::test]
async fn associate_category_with_brand() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;
    let cat_id = create_test_category(&db.pool).await;

    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    service
        .associate_category(&admin, brand_id, cat_id)
        .await
        .unwrap();

    let categories = service.list_categories_for_brand(brand_id).await.unwrap();
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].name, "Electronics");
}

#[tokio::test]
async fn associate_nonexistent_category_fails() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;

    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    let result = service
        .associate_category(&admin, brand_id, uuid::Uuid::new_v4())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn associate_category_requires_admin() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;
    let cat_id = create_test_category(&db.pool).await;

    let service = test_brand_service(db.pool.clone());
    let seller = seller_user();

    let result = service.associate_category(&seller, brand_id, cat_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn disassociate_category_from_brand() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;
    let cat_id = create_test_category(&db.pool).await;
    associate_brand_category(&db.pool, brand_id, cat_id).await;

    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    // Verify it exists
    let categories = service.list_categories_for_brand(brand_id).await.unwrap();
    assert_eq!(categories.len(), 1);

    // Disassociate
    service
        .disassociate_category(&admin, brand_id, cat_id)
        .await
        .unwrap();

    let categories = service.list_categories_for_brand(brand_id).await.unwrap();
    assert!(categories.is_empty());
}

#[tokio::test]
async fn disassociate_nonexistent_association_fails() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;
    let cat_id = create_test_category(&db.pool).await;
    // Not associated

    let service = test_brand_service(db.pool.clone());
    let admin = admin_user();

    let result = service
        .disassociate_category(&admin, brand_id, cat_id)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn list_categories_for_brand_multiple() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;
    let cat1 = create_test_category_named(&db.pool, "Electronics").await;
    let cat2 = create_test_category_named(&db.pool, "Appliances").await;
    associate_brand_category(&db.pool, brand_id, cat1).await;
    associate_brand_category(&db.pool, brand_id, cat2).await;

    let service = test_brand_service(db.pool.clone());
    let categories = service.list_categories_for_brand(brand_id).await.unwrap();
    assert_eq!(categories.len(), 2);

    // Ordered by name ASC
    assert_eq!(categories[0].name, "Appliances");
    assert_eq!(categories[1].name, "Electronics");
}
