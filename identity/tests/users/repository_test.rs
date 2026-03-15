use crate::common::{sample_create_req, sample_create_req_2, sample_update_req};
use chrono::{Duration, Utc};
use identity::users::dtos::{ValidUserCreateReq, ValidUserUpdateReq};
use identity::users::repository::*;
use identity::users::value_objects::{Email, HashedPassword, UserId, Username};
use shared::errors::AppError;
use uuid::Uuid;

fn validated_create(
    req: identity::users::dtos::UserCreateReq,
) -> (ValidUserCreateReq, HashedPassword) {
    let password = HashedPassword::new(req.password.clone());
    let validated: ValidUserCreateReq = req.try_into().expect("sample data should be valid");
    (validated, password)
}

fn validated_update(req: identity::users::dtos::UserUpdateReq) -> ValidUserUpdateReq {
    req.try_into().expect("sample data should be valid")
}

// ── Constraint Tests ─────────────────────────────────────────

#[tokio::test]
async fn create_user_duplicate_username_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req1 = sample_create_req();
    let mut req2 = sample_create_req_2();
    req2.username = req1.username.clone();

    let (v1, p1) = validated_create(req1);
    let mut conn = pool.acquire().await.unwrap();
    let _id = create_user(&mut *conn, v1, &p1).await.unwrap();

    let (v2, p2) = validated_create(req2);
    let mut conn2 = pool.acquire().await.unwrap();
    let result = create_user(&mut *conn2, v2, &p2).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn create_user_duplicate_email_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req1 = sample_create_req();
    let mut req2 = sample_create_req_2();
    req2.email = req1.email.clone();

    let (v1, p1) = validated_create(req1);
    let mut conn = pool.acquire().await.unwrap();
    let _id = create_user(&mut *conn, v1, &p1).await.unwrap();

    let (v2, p2) = validated_create(req2);
    let mut conn2 = pool.acquire().await.unwrap();
    let result = create_user(&mut *conn2, v2, &p2).await;
    assert!(result.is_err());
}

// ── Nonexistent Entity Error Tests ──────────────────────────

#[tokio::test]
async fn get_user_by_id_nonexistent_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let result = get_user_by_id(&pool, UserId::new(Uuid::new_v4())).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_user_by_username_nonexistent_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let result = get_user_by_username(&pool, Username::new("nonexistent").unwrap()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_nonexistent_user_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let validated_update_req = validated_update(sample_update_req());

    let mut conn = pool.acquire().await.unwrap();
    let result = update_user(
        &mut *conn,
        UserId::new(Uuid::new_v4()),
        validated_update_req,
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn delete_nonexistent_user_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let mut conn = pool.acquire().await.unwrap();
    let result = delete_user(&mut *conn, UserId::new(Uuid::new_v4())).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_user_by_email_nonexistent_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let result = get_user_by_email(&pool, Email::new("nonexistent@example.com").unwrap()).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AppError::NotFound(_)));
}

// ── Default Value / SQL Filter Tests ────────────────────────

#[tokio::test]
async fn new_user_has_email_verified_false() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let username = req.username.clone();

    let (validated, password) = validated_create(req);
    let mut conn = pool.acquire().await.unwrap();
    let _id = create_user(&mut *conn, validated, &password).await.unwrap();

    let user = get_user_by_username(&pool, Username::new(&username).unwrap())
        .await
        .unwrap();
    assert!(!user.email_verified);
}

#[tokio::test]
async fn get_expired_verification_token_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (validated, password) = validated_create(sample_create_req());
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let expires_at = Utc::now() - Duration::hours(1); // already expired
    create_verification_token(&mut *conn2, user_id, "expired-token", expires_at)
        .await
        .unwrap();

    let result = get_valid_verification_token(&pool, "expired-token").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_expired_password_reset_token_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (validated, password) = validated_create(sample_create_req());
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let expires_at = Utc::now() - Duration::hours(1); // already expired
    create_password_reset_token(&mut *conn2, user_id, "expired-reset-token", expires_at)
        .await
        .unwrap();

    let result = get_valid_password_reset_token(&pool, "expired-reset-token").await;
    assert!(result.is_err());
}
