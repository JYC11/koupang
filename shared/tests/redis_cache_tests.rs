use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use shared::cache::RedisCache;
use shared::test_utils::redis::TestRedis;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestPayload {
    id: i32,
    name: String,
}

fn sample_payload() -> TestPayload {
    TestPayload {
        id: 42,
        name: "widget".to_string(),
    }
}

// ── Basic Operations ────────────────────────────────────────

#[tokio::test]
async fn get_returns_none_on_cache_miss() {
    let redis = TestRedis::start().await;
    let cache = RedisCache::new(Some(redis.conn), 300);

    let result: Option<TestPayload> = cache.get("nonexistent").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn set_then_get_roundtrips_json() {
    let redis = TestRedis::start().await;
    let cache = RedisCache::new(Some(redis.conn), 300);
    let payload = sample_payload();

    cache.set("key:1", &payload).await;
    let cached: Option<TestPayload> = cache.get("key:1").await;

    assert_eq!(cached, Some(payload));
}

#[tokio::test]
async fn evict_removes_cached_entry() {
    let redis = TestRedis::start().await;
    let cache = RedisCache::new(Some(redis.conn), 300);
    let payload = sample_payload();

    cache.set("key:2", &payload).await;
    assert!(cache.get::<TestPayload>("key:2").await.is_some());

    cache.evict("key:2").await;
    assert!(cache.get::<TestPayload>("key:2").await.is_none());
}

#[tokio::test]
async fn evict_on_nonexistent_key_is_noop() {
    let redis = TestRedis::start().await;
    let cache = RedisCache::new(Some(redis.conn), 300);

    // Should not panic or error
    cache.evict("does-not-exist").await;
}

// ── TTL Behavior ────────────────────────────────────────────

#[tokio::test]
async fn set_applies_ttl_to_key() {
    let redis = TestRedis::start().await;
    let mut assert_conn = redis.conn.clone();
    let cache = RedisCache::new(Some(redis.conn), 120);

    cache.set("ttl-key", &sample_payload()).await;

    let ttl: i64 = assert_conn.ttl("ttl-key").await.unwrap();
    assert!(ttl > 0 && ttl <= 120, "TTL should be set, got {ttl}");
}

// ── Noop / Graceful Degradation ─────────────────────────────

#[tokio::test]
async fn noop_cache_always_misses() {
    let cache = RedisCache::noop();

    cache.set("key", &sample_payload()).await;
    let result: Option<TestPayload> = cache.get("key").await;

    assert!(result.is_none(), "noop cache should never return data");
}

#[tokio::test]
async fn none_conn_gracefully_degrades() {
    let cache = RedisCache::new(None, 300);

    cache.set("key", &sample_payload()).await;
    let result: Option<TestPayload> = cache.get("key").await;
    cache.evict("key").await;

    assert!(result.is_none());
}

// ── Deserialization Mismatch ────────────────────────────────

#[derive(Debug, Deserialize)]
struct DifferentSchema {
    #[allow(dead_code)]
    totally_different_field: Vec<u8>,
}

#[tokio::test]
async fn get_returns_none_on_schema_mismatch() {
    let redis = TestRedis::start().await;
    let cache = RedisCache::new(Some(redis.conn), 300);

    // Store a TestPayload
    cache.set("schema-key", &sample_payload()).await;

    // Try to deserialize as a different type
    let result: Option<DifferentSchema> = cache.get("schema-key").await;
    assert!(result.is_none(), "schema mismatch should return None");
}

// ── Multiple Keys ───────────────────────────────────────────

#[tokio::test]
async fn multiple_keys_are_independent() {
    let redis = TestRedis::start().await;
    let cache = RedisCache::new(Some(redis.conn), 300);

    let p1 = TestPayload {
        id: 1,
        name: "first".into(),
    };
    let p2 = TestPayload {
        id: 2,
        name: "second".into(),
    };

    cache.set("mk:1", &p1).await;
    cache.set("mk:2", &p2).await;

    assert_eq!(cache.get::<TestPayload>("mk:1").await, Some(p1));
    assert_eq!(cache.get::<TestPayload>("mk:2").await, Some(p2));

    cache.evict("mk:1").await;
    assert!(cache.get::<TestPayload>("mk:1").await.is_none());
    assert!(cache.get::<TestPayload>("mk:2").await.is_some());
}

// ── Overwrite ───────────────────────────────────────────────

#[tokio::test]
async fn set_overwrites_existing_value() {
    let redis = TestRedis::start().await;
    let cache = RedisCache::new(Some(redis.conn), 300);

    let original = sample_payload();
    let updated = TestPayload {
        id: 42,
        name: "updated-widget".into(),
    };

    cache.set("ow:1", &original).await;
    cache.set("ow:1", &updated).await;

    let result: Option<TestPayload> = cache.get("ow:1").await;
    assert_eq!(result, Some(updated));
}
