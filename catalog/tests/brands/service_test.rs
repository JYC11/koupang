use crate::common::{
    admin_user, create_test_brand, create_test_category, sample_create_product_with_fks,
    seller_user, test_app_state, test_db,
};
use catalog::brands::service;
use catalog::categories::value_objects::CategoryId;
use catalog::products::service as product_service;

// ── Business guard tests ────────────────────────────────────

#[tokio::test]
async fn delete_brand_with_products_fails() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;

    // Create a product referencing this brand
    let state = test_app_state(db.pool.clone());
    let seller = seller_user();
    product_service::create_product(
        &state,
        &seller,
        sample_create_product_with_fks(None, Some(brand_id)),
    )
    .await
    .unwrap();

    let admin = admin_user();

    let result = service::delete_brand(&state, &admin, brand_id).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("associated products"),
        "Expected products guard, got: {}",
        err
    );
}

#[tokio::test]
async fn associate_nonexistent_category_fails() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let admin = admin_user();

    let result = service::associate_category(
        &state,
        &admin,
        brand_id,
        CategoryId::new(uuid::Uuid::new_v4()),
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn disassociate_nonexistent_association_fails() {
    let db = test_db().await;
    let brand_id = create_test_brand(&db.pool).await;
    let cat_id = create_test_category(&db.pool).await;
    // Not associated

    let state = test_app_state(db.pool.clone());
    let admin = admin_user();

    let result = service::disassociate_category(&state, &admin, brand_id, cat_id).await;
    assert!(result.is_err());
}
