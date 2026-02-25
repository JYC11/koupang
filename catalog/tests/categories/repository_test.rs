use crate::common::test_db;
use catalog::categories::dtos::{ValidCreateCategoryReq, ValidUpdateCategoryReq};
use catalog::categories::repository;
use catalog::categories::value_objects::{CategoryId, CategoryName, LtreeLabel};
use catalog::common::value_objects::Slug;
use uuid::Uuid;

fn sample_create_category(name: &str, parent_id: Option<CategoryId>) -> ValidCreateCategoryReq {
    ValidCreateCategoryReq {
        name: CategoryName::new(name).unwrap(),
        slug: Slug::from_name(name).unwrap(),
        label: LtreeLabel::from_name(name).unwrap(),
        parent_id,
        description: Some(format!("{} description", name)),
    }
}

/// Helper: create a root category and return (id, path).
async fn create_root(db: &shared::test_utils::db::TestDb, name: &str) -> (CategoryId, String) {
    let req = sample_create_category(name, None);
    let path = req.label.as_str().to_string();
    let mut conn = db.pool.acquire().await.unwrap();
    let id = repository::create_category(&mut *conn, req, &path, 0)
        .await
        .unwrap();
    (id, path)
}

/// Helper: create a child category and return (id, path).
async fn create_child(
    db: &shared::test_utils::db::TestDb,
    name: &str,
    parent_id: CategoryId,
    parent_path: &str,
    depth: i32,
) -> (CategoryId, String) {
    let req = sample_create_category(name, Some(parent_id));
    let path = format!("{}.{}", parent_path, req.label.as_str());
    let mut conn = db.pool.acquire().await.unwrap();
    let id = repository::create_category(&mut *conn, req, &path, depth)
        .await
        .unwrap();
    (id, path)
}

// ── CRUD tests ──────────────────────────────────────────────

#[tokio::test]
async fn create_and_get_category() {
    let db = test_db().await;
    let (cat_id, _) = create_root(&db, "Electronics").await;

    let cat = repository::get_category_by_id(&db.pool, cat_id)
        .await
        .unwrap();

    assert_eq!(cat.id, cat_id.value());
    assert_eq!(cat.name, "Electronics");
    assert_eq!(cat.slug, "electronics");
    assert_eq!(cat.path, "electronics");
    assert_eq!(cat.depth, 0);
    assert!(cat.parent_id.is_none());
    assert_eq!(cat.description.as_deref(), Some("Electronics description"));
}

#[tokio::test]
async fn get_category_by_slug() {
    let db = test_db().await;
    create_root(&db, "Clothing").await;

    let cat = repository::get_category_by_slug(&db.pool, "clothing")
        .await
        .unwrap();

    assert_eq!(cat.name, "Clothing");
}

#[tokio::test]
async fn get_nonexistent_category_returns_error() {
    let db = test_db().await;
    let result = repository::get_category_by_id(&db.pool, CategoryId::new(Uuid::new_v4())).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_category_by_nonexistent_slug_returns_error() {
    let db = test_db().await;
    let result = repository::get_category_by_slug(&db.pool, "no-such-category").await;
    assert!(result.is_err());
}

// ── List tests ──────────────────────────────────────────────

#[tokio::test]
async fn list_root_categories() {
    let db = test_db().await;
    create_root(&db, "Books").await;
    create_root(&db, "Appliances").await;

    let roots = repository::list_root_categories(&db.pool).await.unwrap();

    assert_eq!(roots.len(), 2);
    // Alphabetical order
    assert_eq!(roots[0].name, "Appliances");
    assert_eq!(roots[1].name, "Books");
}

#[tokio::test]
async fn list_root_categories_excludes_children() {
    let db = test_db().await;
    let (root_id, root_path) = create_root(&db, "Electronics").await;
    create_child(&db, "Phones", root_id, &root_path, 1).await;

    let roots = repository::list_root_categories(&db.pool).await.unwrap();

    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].name, "Electronics");
}

// ── Hierarchy tests ─────────────────────────────────────────

#[tokio::test]
async fn create_child_category() {
    let db = test_db().await;
    let (root_id, root_path) = create_root(&db, "Electronics").await;
    let (child_id, _) = create_child(&db, "Phones", root_id, &root_path, 1).await;

    let child = repository::get_category_by_id(&db.pool, child_id)
        .await
        .unwrap();

    assert_eq!(child.name, "Phones");
    assert_eq!(child.path, "electronics.phones");
    assert_eq!(child.depth, 1);
    assert_eq!(child.parent_id, Some(root_id.value()));
}

#[tokio::test]
async fn get_children() {
    let db = test_db().await;
    let (root_id, root_path) = create_root(&db, "Electronics").await;
    create_child(&db, "Tablets", root_id, &root_path, 1).await;
    create_child(&db, "Phones", root_id, &root_path, 1).await;

    let children = repository::get_children(&db.pool, root_id).await.unwrap();

    assert_eq!(children.len(), 2);
    // Alphabetical order
    assert_eq!(children[0].name, "Phones");
    assert_eq!(children[1].name, "Tablets");
}

#[tokio::test]
async fn get_children_empty() {
    let db = test_db().await;
    let (leaf_id, _) = create_root(&db, "Leaf Category").await;

    let children = repository::get_children(&db.pool, leaf_id).await.unwrap();
    assert!(children.is_empty());
}

#[tokio::test]
async fn get_subtree() {
    let db = test_db().await;
    let (root_id, root_path) = create_root(&db, "Electronics").await;
    let (child_id, child_path) = create_child(&db, "Phones", root_id, &root_path, 1).await;
    create_child(&db, "Smartphones", child_id, &child_path, 2).await;

    let subtree = repository::get_subtree(&db.pool, &root_path).await.unwrap();

    assert_eq!(subtree.len(), 3);
    // Ordered by path
    assert_eq!(subtree[0].name, "Electronics");
    assert_eq!(subtree[1].name, "Phones");
    assert_eq!(subtree[2].name, "Smartphones");
}

#[tokio::test]
async fn get_ancestors() {
    let db = test_db().await;
    let (root_id, root_path) = create_root(&db, "Electronics").await;
    let (child_id, child_path) = create_child(&db, "Phones", root_id, &root_path, 1).await;
    let (_, grandchild_path) = create_child(&db, "Smartphones", child_id, &child_path, 2).await;

    let ancestors = repository::get_ancestors(&db.pool, &grandchild_path)
        .await
        .unwrap();

    assert_eq!(ancestors.len(), 3);
    // Ordered by depth
    assert_eq!(ancestors[0].name, "Electronics");
    assert_eq!(ancestors[1].name, "Phones");
    assert_eq!(ancestors[2].name, "Smartphones");
}

// ── Update tests ────────────────────────────────────────────

#[tokio::test]
async fn update_category_name() {
    let db = test_db().await;
    let (cat_id, _) = create_root(&db, "Old Name").await;

    let update = ValidUpdateCategoryReq {
        name: Some(CategoryName::new("New Name").unwrap()),
        description: None,
    };
    let mut conn = db.pool.acquire().await.unwrap();
    repository::update_category(&mut *conn, cat_id, update)
        .await
        .unwrap();

    let cat = repository::get_category_by_id(&db.pool, cat_id)
        .await
        .unwrap();
    assert_eq!(cat.name, "New Name");
    assert!(cat.updated_at.is_some());
}

#[tokio::test]
async fn update_category_description() {
    let db = test_db().await;
    let (cat_id, _) = create_root(&db, "Test Category").await;

    let update = ValidUpdateCategoryReq {
        name: None,
        description: Some("Updated description".to_string()),
    };
    let mut conn = db.pool.acquire().await.unwrap();
    repository::update_category(&mut *conn, cat_id, update)
        .await
        .unwrap();

    let cat = repository::get_category_by_id(&db.pool, cat_id)
        .await
        .unwrap();
    assert_eq!(cat.description.as_deref(), Some("Updated description"));
}

#[tokio::test]
async fn update_nonexistent_category_returns_error() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();

    let update = ValidUpdateCategoryReq {
        name: Some(CategoryName::new("Ghost").unwrap()),
        description: None,
    };
    let result =
        repository::update_category(&mut *conn, CategoryId::new(Uuid::new_v4()), update).await;
    assert!(result.is_err());
}

// ── Delete tests ────────────────────────────────────────────

#[tokio::test]
async fn delete_category() {
    let db = test_db().await;
    let (cat_id, _) = create_root(&db, "Doomed Category").await;

    let mut conn = db.pool.acquire().await.unwrap();
    repository::delete_category(&mut *conn, cat_id)
        .await
        .unwrap();

    let result = repository::get_category_by_id(&db.pool, cat_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn delete_nonexistent_category_returns_error() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let result = repository::delete_category(&mut *conn, CategoryId::new(Uuid::new_v4())).await;
    assert!(result.is_err());
}

// ── has_children / has_products ─────────────────────────────

#[tokio::test]
async fn has_children_true_and_false() {
    let db = test_db().await;
    let (root_id, root_path) = create_root(&db, "Electronics").await;

    // No children yet
    assert!(!repository::has_children(&db.pool, root_id).await.unwrap());

    // Add a child
    create_child(&db, "Phones", root_id, &root_path, 1).await;

    assert!(repository::has_children(&db.pool, root_id).await.unwrap());
}

#[tokio::test]
async fn has_products_false_when_none() {
    let db = test_db().await;
    let (cat_id, _) = create_root(&db, "Empty Category").await;

    let result = repository::has_products(&db.pool, cat_id).await.unwrap();
    assert!(!result);
}
