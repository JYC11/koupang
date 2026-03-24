use std::time::Duration;

use shared::distributed_lock::{DistributedLock, LockError, RetryConfig};
use shared::test_utils::redis::TestRedis;

async fn test_lock() -> (DistributedLock, redis::aio::ConnectionManager) {
    let redis = TestRedis::start().await;
    let lock = DistributedLock::new(redis.conn.clone());
    (lock, redis.conn)
}

// ── Acquire ─────────────────────────────────────────────────

#[tokio::test]
async fn acquire_succeeds_on_free_key() {
    let (lock, _) = test_lock().await;
    let guard = lock.acquire("test:free", Duration::from_secs(5)).await;
    assert!(guard.is_ok());
}

#[tokio::test]
async fn acquire_fails_when_key_already_held() {
    let (lock, _) = test_lock().await;

    let _guard = lock
        .acquire("test:held", Duration::from_secs(10))
        .await
        .unwrap();

    let result = lock.acquire("test:held", Duration::from_secs(5)).await;
    assert!(matches!(result, Err(LockError::AlreadyHeld)));
}

#[tokio::test]
async fn acquire_succeeds_after_ttl_expires() {
    let (lock, _) = test_lock().await;

    let _guard = lock
        .acquire("test:ttl", Duration::from_secs(1))
        .await
        .unwrap();

    // Wait for TTL to expire.
    tokio::time::sleep(Duration::from_millis(1100)).await;

    let result = lock.acquire("test:ttl", Duration::from_secs(5)).await;
    assert!(result.is_ok(), "should acquire after TTL expiry");
}

// ── Release ─────────────────────────────────────────────────

#[tokio::test]
async fn release_with_correct_token_succeeds() {
    let (lock, _) = test_lock().await;

    let guard = lock
        .acquire("test:release", Duration::from_secs(10))
        .await
        .unwrap();
    let released = guard.release().await.unwrap();
    assert!(released, "should return true when we held the lock");
}

#[tokio::test]
async fn release_after_expiry_returns_false() {
    let (lock, _) = test_lock().await;

    let guard = lock
        .acquire("test:expired", Duration::from_secs(1))
        .await
        .unwrap();

    // Wait for TTL to expire.
    tokio::time::sleep(Duration::from_millis(1100)).await;

    let released = guard.release().await.unwrap();
    assert!(!released, "should return false when lock expired");
}

#[tokio::test]
async fn release_does_not_delete_other_holders_lock() {
    let (lock, _) = test_lock().await;

    // Holder A acquires with short TTL.
    let guard_a = lock
        .acquire("test:steal", Duration::from_secs(1))
        .await
        .unwrap();

    // Wait for A's lock to expire.
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // Holder B acquires the same key.
    let _guard_b = lock
        .acquire("test:steal", Duration::from_secs(10))
        .await
        .unwrap();

    // A tries to release — should NOT delete B's lock.
    let released = guard_a.release().await.unwrap();
    assert!(!released, "A should not release B's lock");

    // B's lock should still be held.
    let result = lock.acquire("test:steal", Duration::from_secs(5)).await;
    assert!(
        matches!(result, Err(LockError::AlreadyHeld)),
        "B's lock should still be held"
    );
}

#[tokio::test]
async fn key_is_free_after_release() {
    let (lock, _) = test_lock().await;

    let guard = lock
        .acquire("test:reacquire", Duration::from_secs(10))
        .await
        .unwrap();
    guard.release().await.unwrap();

    // Should be able to re-acquire.
    let result = lock.acquire("test:reacquire", Duration::from_secs(5)).await;
    assert!(result.is_ok(), "should acquire after explicit release");
}

// ── Retry ───────────────────────────────────────────────────

#[tokio::test]
async fn acquire_with_retry_succeeds_on_first_try() {
    let (lock, _) = test_lock().await;

    let guard = lock
        .acquire_with_retry(
            "test:retry-free",
            Duration::from_secs(5),
            RetryConfig {
                max_attempts: 3,
                retry_delay: Duration::from_millis(50),
            },
        )
        .await;

    assert!(guard.is_ok());
}

#[tokio::test]
async fn acquire_with_retry_succeeds_after_expiry() {
    let (lock, _) = test_lock().await;

    // Hold the lock with a 1-second TTL.
    let _guard = lock
        .acquire("test:retry-wait", Duration::from_secs(1))
        .await
        .unwrap();

    // Retry with delays that span the TTL expiry.
    let guard = lock
        .acquire_with_retry(
            "test:retry-wait",
            Duration::from_secs(5),
            RetryConfig {
                max_attempts: 10,
                retry_delay: Duration::from_millis(200),
            },
        )
        .await;

    assert!(guard.is_ok(), "should succeed after TTL expiry");
}

#[tokio::test]
async fn acquire_with_retry_exhausts_attempts() {
    let (lock, _) = test_lock().await;

    // Hold the lock with a long TTL.
    let _guard = lock
        .acquire("test:retry-fail", Duration::from_secs(30))
        .await
        .unwrap();

    let result = lock
        .acquire_with_retry(
            "test:retry-fail",
            Duration::from_secs(5),
            RetryConfig {
                max_attempts: 3,
                retry_delay: Duration::from_millis(50),
            },
        )
        .await;

    assert!(matches!(result, Err(LockError::AlreadyHeld)));
}

// ── Guard key accessor ──────────────────────────────────────

#[tokio::test]
async fn guard_key_returns_lock_key() {
    let (lock, _) = test_lock().await;
    let guard = lock
        .acquire("test:key-check", Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(guard.key(), "test:key-check");
}
