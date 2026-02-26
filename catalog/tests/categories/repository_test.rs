use crate::common::test_db;
use catalog::categories::dtos::ValidCreateCategoryReq;
use catalog::categories::repository;
use catalog::categories::value_objects::{CategoryId, CategoryName, LtreeLabel};
use catalog::common::value_objects::Slug;

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

// ── Postgres ltree path correctness ─────────────────────────

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

// ── Internal helper tests ───────────────────────────────────

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
