use crate::common::{
    admin_user, create_test_category, create_test_category_named, create_test_child_category,
    sample_create_product_with_fks, seller_user, test_catalog_service, test_category_service,
    test_db,
};
use catalog::categories::dtos::{CreateCategoryReq, UpdateCategoryReq};

// ── Create ──────────────────────────────────────────────────

#[tokio::test]
async fn create_root_category() {
    let db = test_db().await;
    let service = test_category_service(db.pool.clone());
    let admin = admin_user();

    let req = CreateCategoryReq {
        name: "Electronics".to_string(),
        slug: None,
        parent_id: None,
        description: Some("Electronic devices".to_string()),
    };

    let category = service.create_category(&admin, req).await.unwrap();
    assert_eq!(category.name, "Electronics");
    assert_eq!(category.slug, "electronics");
    assert_eq!(category.depth, 0);
    assert!(category.parent_id.is_none());
    assert_eq!(category.description.as_deref(), Some("Electronic devices"));
}

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

#[tokio::test]
async fn create_category_requires_admin() {
    let db = test_db().await;
    let service = test_category_service(db.pool.clone());
    let seller = seller_user();

    let req = CreateCategoryReq {
        name: "Hacked".to_string(),
        slug: None,
        parent_id: None,
        description: None,
    };

    let result = service.create_category(&seller, req).await;
    assert!(result.is_err());
}

// ── Read ────────────────────────────────────────────────────

#[tokio::test]
async fn get_category_by_id() {
    let db = test_db().await;
    let service = test_category_service(db.pool.clone());
    let admin = admin_user();

    let created = service
        .create_category(
            &admin,
            CreateCategoryReq {
                name: "Books".to_string(),
                slug: None,
                parent_id: None,
                description: None,
            },
        )
        .await
        .unwrap();

    let id = uuid::Uuid::parse_str(&created.id).unwrap();
    let fetched = service.get_category(id).await.unwrap();
    assert_eq!(fetched.name, "Books");
    assert_eq!(fetched.id, created.id);
}

#[tokio::test]
async fn get_category_by_slug() {
    let db = test_db().await;
    let service = test_category_service(db.pool.clone());
    let admin = admin_user();

    service
        .create_category(
            &admin,
            CreateCategoryReq {
                name: "Home Garden".to_string(),
                slug: None,
                parent_id: None,
                description: None,
            },
        )
        .await
        .unwrap();

    let fetched = service.get_category_by_slug("home-garden").await.unwrap();
    assert_eq!(fetched.name, "Home Garden");
}

#[tokio::test]
async fn get_nonexistent_category_fails() {
    let db = test_db().await;
    let service = test_category_service(db.pool.clone());

    let result = service.get_category(uuid::Uuid::new_v4()).await;
    assert!(result.is_err());
}

// ── List / Tree queries ─────────────────────────────────────

#[tokio::test]
async fn list_root_categories_excludes_children() {
    let db = test_db().await;
    let root_id = create_test_category_named(&db.pool, "Electronics").await;
    create_test_child_category(&db.pool, root_id, "Smartphones").await;
    create_test_category_named(&db.pool, "Books").await;

    let service = test_category_service(db.pool.clone());
    let roots = service.list_root_categories().await.unwrap();
    assert_eq!(roots.len(), 2);

    let names: Vec<&str> = roots.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Books"));
    assert!(names.contains(&"Electronics"));
    // Smartphones is NOT a root
    assert!(!names.contains(&"Smartphones"));
}

#[tokio::test]
async fn get_children_returns_direct_children() {
    let db = test_db().await;
    let root_id = create_test_category_named(&db.pool, "Electronics").await;
    let phones_id = create_test_child_category(&db.pool, root_id, "Phones").await;
    create_test_child_category(&db.pool, root_id, "Laptops").await;
    // Grandchild should NOT appear
    create_test_child_category(&db.pool, phones_id, "Android").await;

    let service = test_category_service(db.pool.clone());
    let children = service.get_children(root_id).await.unwrap();
    assert_eq!(children.len(), 2);

    let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Phones"));
    assert!(names.contains(&"Laptops"));
    assert!(!names.contains(&"Android"));
}

#[tokio::test]
async fn get_subtree_returns_all_descendants() {
    let db = test_db().await;
    let root_id = create_test_category_named(&db.pool, "Electronics").await;
    let phones_id = create_test_child_category(&db.pool, root_id, "Phones").await;
    create_test_child_category(&db.pool, phones_id, "Android").await;

    let service = test_category_service(db.pool.clone());
    let subtree = service.get_subtree(root_id).await.unwrap();
    // Should include root + Phones + Android
    assert_eq!(subtree.len(), 3);
}

#[tokio::test]
async fn get_ancestors_returns_path_to_root() {
    let db = test_db().await;
    let root_id = create_test_category_named(&db.pool, "Electronics").await;
    let phones_id = create_test_child_category(&db.pool, root_id, "Phones").await;
    let android_id = create_test_child_category(&db.pool, phones_id, "Android").await;

    let service = test_category_service(db.pool.clone());
    let ancestors = service.get_ancestors(android_id).await.unwrap();
    // Should include Electronics, Phones, Android (ordered by depth ASC)
    assert_eq!(ancestors.len(), 3);
    assert_eq!(ancestors[0].name, "Electronics");
    assert_eq!(ancestors[1].name, "Phones");
    assert_eq!(ancestors[2].name, "Android");
}

// ── Update ──────────────────────────────────────────────────

#[tokio::test]
async fn update_category_name() {
    let db = test_db().await;
    let service = test_category_service(db.pool.clone());
    let admin = admin_user();

    let created = service
        .create_category(
            &admin,
            CreateCategoryReq {
                name: "Old Name".to_string(),
                slug: None,
                parent_id: None,
                description: None,
            },
        )
        .await
        .unwrap();

    let id = uuid::Uuid::parse_str(&created.id).unwrap();
    service
        .update_category(
            &admin,
            id,
            UpdateCategoryReq {
                name: Some("New Name".to_string()),
                description: None,
            },
        )
        .await
        .unwrap();

    let updated = service.get_category(id).await.unwrap();
    assert_eq!(updated.name, "New Name");
}

#[tokio::test]
async fn update_category_requires_admin() {
    let db = test_db().await;
    let cat_id = create_test_category(&db.pool).await;
    let service = test_category_service(db.pool.clone());
    let seller = seller_user();

    let result = service
        .update_category(
            &seller,
            cat_id,
            UpdateCategoryReq {
                name: Some("Hacked".to_string()),
                description: None,
            },
        )
        .await;
    assert!(result.is_err());
}

// ── Delete ──────────────────────────────────────────────────

#[tokio::test]
async fn delete_leaf_category() {
    let db = test_db().await;
    let service = test_category_service(db.pool.clone());
    let admin = admin_user();

    let created = service
        .create_category(
            &admin,
            CreateCategoryReq {
                name: "Temporary".to_string(),
                slug: None,
                parent_id: None,
                description: None,
            },
        )
        .await
        .unwrap();

    let id = uuid::Uuid::parse_str(&created.id).unwrap();
    service.delete_category(&admin, id).await.unwrap();

    let result = service.get_category(id).await;
    assert!(result.is_err());
}

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

#[tokio::test]
async fn delete_category_requires_admin() {
    let db = test_db().await;
    let cat_id = create_test_category(&db.pool).await;
    let service = test_category_service(db.pool.clone());
    let seller = seller_user();

    let result = service.delete_category(&seller, cat_id).await;
    assert!(result.is_err());
}
