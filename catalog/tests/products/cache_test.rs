use crate::common::{
    sample_create_product_req, sample_create_sku_req, seller_user, test_app_state, test_db,
};
use catalog::AppState;
use catalog::products::dtos::UpdateProductReq;
use catalog::products::service;
use catalog::products::value_objects::ProductId;
use redis::AsyncCommands;
use shared::cache::RedisCache;
use shared::test_utils::auth::test_auth_config;
use shared::test_utils::redis::TestRedis;

fn test_app_state_with_redis(
    pool: shared::db::PgPool,
    redis_conn: redis::aio::ConnectionManager,
) -> AppState {
    AppState {
        pool,
        cache: RedisCache::new(Some(redis_conn), 300),
        auth_config: test_auth_config(),
    }
}

// ── Cache hit tests ─────────────────────────────────────────

#[tokio::test]
async fn get_product_detail_caches_on_first_call() {
    let db = test_db().await;
    let redis = TestRedis::start().await;
    let state = test_app_state_with_redis(db.pool.clone(), redis.conn.clone());
    let seller = seller_user();

    let product = service::create_product(&state, &seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());

    // First call — cache miss, populates cache
    let detail1 = service::get_product_detail(&state, product_id)
        .await
        .unwrap();

    // Verify cache key exists in Redis
    let key = format!("product:{}", product.id);
    let cached: Option<String> = redis.conn.clone().get(&key).await.unwrap();
    assert!(cached.is_some(), "expected product detail to be cached");

    // Second call — cache hit (same result)
    let detail2 = service::get_product_detail(&state, product_id)
        .await
        .unwrap();
    assert_eq!(detail1.product.id, detail2.product.id);
    assert_eq!(detail1.product.name, detail2.product.name);
}

#[tokio::test]
async fn get_product_by_slug_caches_on_first_call() {
    let db = test_db().await;
    let redis = TestRedis::start().await;
    let state = test_app_state_with_redis(db.pool.clone(), redis.conn.clone());
    let seller = seller_user();

    let product = service::create_product(&state, &seller, sample_create_product_req())
        .await
        .unwrap();

    // First call — cache miss
    let res1 = service::get_product_by_slug(&state, &product.slug)
        .await
        .unwrap();

    // Verify cache key exists
    let key = format!("product:slug:{}", product.slug);
    let cached: Option<String> = redis.conn.clone().get(&key).await.unwrap();
    assert!(cached.is_some(), "expected slug cache entry");

    // Second call — cache hit
    let res2 = service::get_product_by_slug(&state, &product.slug)
        .await
        .unwrap();
    assert_eq!(res1.id, res2.id);
}

// ── Cache eviction tests ────────────────────────────────────

#[tokio::test]
async fn update_product_evicts_caches() {
    let db = test_db().await;
    let redis = TestRedis::start().await;
    let state = test_app_state_with_redis(db.pool.clone(), redis.conn.clone());
    let seller = seller_user();

    let product = service::create_product(&state, &seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());

    // Populate both caches
    service::get_product_detail(&state, product_id)
        .await
        .unwrap();
    service::get_product_by_slug(&state, &product.slug)
        .await
        .unwrap();

    // Update the product
    service::update_product(
        &state,
        &seller,
        product_id,
        UpdateProductReq {
            name: Some("Updated Name".to_string()),
            slug: None,
            description: None,
            base_price: None,
            currency: None,
            category_id: None,
            brand_id: None,
            status: None,
        },
    )
    .await
    .unwrap();

    // Both caches should be evicted
    let detail_key = format!("product:{}", product.id);
    let slug_key = format!("product:slug:{}", product.slug);
    let detail_cached: Option<String> = redis.conn.clone().get(&detail_key).await.unwrap();
    let slug_cached: Option<String> = redis.conn.clone().get(&slug_key).await.unwrap();
    assert!(detail_cached.is_none(), "detail cache should be evicted");
    assert!(slug_cached.is_none(), "slug cache should be evicted");
}

#[tokio::test]
async fn delete_product_evicts_caches() {
    let db = test_db().await;
    let redis = TestRedis::start().await;
    let state = test_app_state_with_redis(db.pool.clone(), redis.conn.clone());
    let seller = seller_user();

    let product = service::create_product(&state, &seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());

    // Populate cache
    service::get_product_detail(&state, product_id)
        .await
        .unwrap();

    // Delete the product
    service::delete_product(&state, &seller, product_id)
        .await
        .unwrap();

    let key = format!("product:{}", product.id);
    let cached: Option<String> = redis.conn.clone().get(&key).await.unwrap();
    assert!(cached.is_none(), "detail cache should be evicted on delete");
}

#[tokio::test]
async fn create_sku_evicts_product_detail_cache() {
    let db = test_db().await;
    let redis = TestRedis::start().await;
    let state = test_app_state_with_redis(db.pool.clone(), redis.conn.clone());
    let seller = seller_user();

    let product = service::create_product(&state, &seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());

    // Populate detail cache (0 SKUs)
    let detail = service::get_product_detail(&state, product_id)
        .await
        .unwrap();
    assert!(detail.skus.is_empty());

    // Create a SKU — should evict detail cache
    service::create_sku(&state, &seller, product_id, sample_create_sku_req())
        .await
        .unwrap();

    let key = format!("product:{}", product.id);
    let cached: Option<String> = redis.conn.clone().get(&key).await.unwrap();
    assert!(
        cached.is_none(),
        "detail cache should be evicted on SKU create"
    );

    // Re-fetch — should now include the SKU
    let detail = service::get_product_detail(&state, product_id)
        .await
        .unwrap();
    assert_eq!(detail.skus.len(), 1);
}

#[tokio::test]
async fn without_redis_reads_work_normally() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let seller = seller_user();

    let product = service::create_product(&state, &seller, sample_create_product_req())
        .await
        .unwrap();
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());

    // Should work fine without Redis (no cache, straight to DB)
    let detail = service::get_product_detail(&state, product_id)
        .await
        .unwrap();
    assert_eq!(detail.product.name, "Test Widget");

    let by_slug = service::get_product_by_slug(&state, &product.slug)
        .await
        .unwrap();
    assert_eq!(by_slug.id, product.id);
}
