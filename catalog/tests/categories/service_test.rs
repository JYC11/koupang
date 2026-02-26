use crate::common::{
    admin_user, create_test_category, create_test_category_named, create_test_child_category,
    sample_create_product_with_fks, seller_user, test_catalog_service, test_category_service,
    test_db,
};
use catalog::categories::dtos::CreateCategoryReq;

// ── Hierarchy business logic ────────────────────────────────

#[tokio::test]
async fn create_child_category_with_parent() {
    let db = test_db().await;
    let service = test_category_service(db.pool.clone());
    let admin = admin_user();

    // Create root
    let root = service
        .create_category(
            &admin,
            CreateCategoryReq {
                name: "Electronics".to_string(),
                slug: None,
                parent_id: None,
                description: None,
            },
        )
        .await
        .unwrap();

    let root_id = uuid::Uuid::parse_str(&root.id).unwrap();

    // Create child
    let child = service
        .create_category(
            &admin,
            CreateCategoryReq {
                name: "Smartphones".to_string(),
                slug: None,
                parent_id: Some(root_id),
                description: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(child.name, "Smartphones");
    assert_eq!(child.depth, 1);
    assert_eq!(child.parent_id.as_deref(), Some(root.id.as_str()));
    assert!(child.path.contains("electronics"));
    assert!(child.path.contains("smartphones"));
}

#[tokio::test]
async fn create_category_with_nonexistent_parent_fails() {
    let db = test_db().await;
    let service = test_category_service(db.pool.clone());
    let admin = admin_user();

    let req = CreateCategoryReq {
        name: "Orphan".to_string(),
        slug: None,
        parent_id: Some(uuid::Uuid::new_v4()),
        description: None,
    };

    let result = service.create_category(&admin, req).await;
    assert!(result.is_err());
}

// ── Delete guard tests ──────────────────────────────────────

#[tokio::test]
async fn delete_category_with_children_fails() {
    let db = test_db().await;
    let root_id = create_test_category_named(&db.pool, "Electronics").await;
    create_test_child_category(&db.pool, root_id, "Phones").await;

    let service = test_category_service(db.pool.clone());
    let admin = admin_user();

    let result = service.delete_category(&admin, root_id).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("child categories"),
        "Expected child categories guard, got: {}",
        err
    );
}

#[tokio::test]
async fn delete_category_with_products_fails() {
    let db = test_db().await;
    let cat_id = create_test_category(&db.pool).await;

    // Create a product referencing this category
    let catalog = test_catalog_service(db.pool.clone());
    let seller = seller_user();
    catalog
        .create_product(&seller, sample_create_product_with_fks(Some(cat_id), None))
        .await
        .unwrap();

    let service = test_category_service(db.pool.clone());
    let admin = admin_user();

    let result = service.delete_category(&admin, cat_id).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("associated products"),
        "Expected products guard, got: {}",
        err
    );
}
