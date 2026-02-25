use crate::common::{sample_create_req, sample_create_req_2, sample_update_req};
use chrono::{Duration, Utc};
use identity::users::dtos::{ValidUserCreateReq, ValidUserUpdateReq};
use identity::users::repository::*;
use identity::users::value_objects::{Email, EmailTokenId, PasswordTokenId, UserId, Username};
use shared::errors::AppError;
use uuid::Uuid;

fn validated_create(req: identity::users::dtos::UserCreateReq) -> (ValidUserCreateReq, String) {
    let password = req.password.clone();
    let validated: ValidUserCreateReq = req.try_into().expect("sample data should be valid");
    (validated, password)
}

fn validated_update(req: identity::users::dtos::UserUpdateReq) -> ValidUserUpdateReq {
    req.try_into().expect("sample data should be valid")
}

#[tokio::test]
async fn create_user_inserts_row() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let username = Username::new(&*req.username).unwrap();
    let email = req.email.clone();
    let phone = req.phone.clone();
    let role = req.role.clone();

    let (validated, password) = validated_create(req);
    let mut conn = pool.acquire().await.unwrap();
    let _id = create_user(&mut *conn, validated, &password).await.unwrap();

    let user = get_user_by_username(&pool, username.clone()).await.unwrap();
    assert_eq!(user.username, username.to_string());
    assert_eq!(user.email, email);
    assert_eq!(user.phone, phone);
    assert_eq!(user.role, role);
    assert!(user.deleted_at.is_none());
}

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

#[tokio::test]
async fn get_user_by_id_returns_existing() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let username = req.username.clone();

    let (validated, password) = validated_create(req);
    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, validated, &password).await.unwrap();

    let created = get_user_by_username(&pool, Username::new(&username).unwrap())
        .await
        .unwrap();
    let fetched = get_user_by_id(&pool, UserId::new(created.id))
        .await
        .unwrap();

    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.username, created.username);
    assert_eq!(fetched.email, created.email);
}

#[tokio::test]
async fn get_user_by_id_nonexistent_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let result = get_user_by_id(&pool, UserId::new(Uuid::new_v4())).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_user_by_username_returns_existing() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let username = req.username.clone();

    let (validated, password) = validated_create(req);
    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, validated, &password).await.unwrap();

    let user = get_user_by_username(&pool, Username::new(&username).unwrap())
        .await
        .unwrap();
    assert_eq!(user.username, username);
}

#[tokio::test]
async fn get_user_by_username_nonexistent_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let result = get_user_by_username(&pool, Username::new("nonexistent").unwrap()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_user_modifies_fields() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let username = req.username.clone();

    let (validated, password) = validated_create(req);
    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, validated, &password).await.unwrap();

    let created = get_user_by_username(&pool, Username::new(&username).unwrap())
        .await
        .unwrap();
    let update_req = sample_update_req();
    let new_username = update_req.username.clone();
    let new_email = update_req.email.clone();
    let new_phone = update_req.phone.clone();
    let validated_update_req = validated_update(update_req);

    let mut conn2 = pool.acquire().await.unwrap();
    update_user(&mut *conn2, UserId::new(created.id), validated_update_req)
        .await
        .unwrap();

    let updated = get_user_by_id(&pool, UserId::new(created.id))
        .await
        .unwrap();
    assert_eq!(updated.username, new_username);
    assert_eq!(updated.email, new_email);
    assert_eq!(updated.phone, new_phone);
    assert!(updated.updated_at.is_some());
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
async fn delete_user_soft_deletes() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let username = req.username.clone();

    let (validated, password) = validated_create(req);
    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, validated, &password).await.unwrap();

    let created = get_user_by_username(&pool, Username::new(&username).unwrap())
        .await
        .unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    delete_user(&mut *conn2, UserId::new(created.id))
        .await
        .unwrap();

    // get_user_by_id filters deleted_at IS NULL, so should return NotFound
    let result = get_user_by_id(&pool, UserId::new(created.id)).await;
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

// ── Email Verification Token Tests ──────────────────────────

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
async fn create_verification_token_inserts_row() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (validated, password) = validated_create(sample_create_req());
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let expires_at = Utc::now() + Duration::hours(24);
    create_verification_token(&mut *conn2, user_id, "test-token-abc", expires_at)
        .await
        .unwrap();

    let token_entity = get_valid_verification_token(&pool, "test-token-abc")
        .await
        .unwrap();
    assert_eq!(token_entity.user_id, user_id.value());
    assert_eq!(token_entity.token, "test-token-abc");
    assert!(token_entity.used_at.is_none());
}

#[tokio::test]
async fn get_valid_verification_token_works() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (validated, password) = validated_create(sample_create_req());
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let expires_at = Utc::now() + Duration::hours(24);
    create_verification_token(&mut *conn2, user_id, "valid-token", expires_at)
        .await
        .unwrap();

    let result = get_valid_verification_token(&pool, "valid-token").await;
    assert!(result.is_ok());
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
async fn mark_token_used_sets_used_at() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (validated, password) = validated_create(sample_create_req());
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let expires_at = Utc::now() + Duration::hours(24);
    create_verification_token(&mut *conn2, user_id, "use-me-token", expires_at)
        .await
        .unwrap();

    let token_entity = get_valid_verification_token(&pool, "use-me-token")
        .await
        .unwrap();

    let mut conn3 = pool.acquire().await.unwrap();
    mark_token_used(&mut *conn3, EmailTokenId::new(token_entity.id))
        .await
        .unwrap();

    // Token should no longer be valid (used_at is set)
    let result = get_valid_verification_token(&pool, "use-me-token").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn verify_user_email_sets_flag() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let username = req.username.clone();
    let (validated, password) = validated_create(req);
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    verify_user_email(&mut *conn2, user_id).await.unwrap();

    let user = get_user_by_username(&pool, Username::new(&username).unwrap())
        .await
        .unwrap();
    assert!(user.email_verified);
    assert!(user.updated_at.is_some());
}

// ── Password Reset Token Tests ──────────────────────────────

#[tokio::test]
async fn get_user_by_email_returns_existing() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let email = req.email.clone();

    let (validated, password) = validated_create(req);
    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, validated, &password).await.unwrap();

    let user = get_user_by_email(&pool, Email::new(&email).unwrap())
        .await
        .unwrap();
    assert_eq!(user.email, email);
}

#[tokio::test]
async fn get_user_by_email_nonexistent_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let result = get_user_by_email(&pool, Email::new("nonexistent@example.com").unwrap()).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AppError::NotFound(_)));
}

#[tokio::test]
async fn create_password_reset_token_inserts_row() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (validated, password) = validated_create(sample_create_req());
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let expires_at = Utc::now() + Duration::hours(24);
    create_password_reset_token(&mut *conn2, user_id, "reset-token-abc", expires_at)
        .await
        .unwrap();

    let token_entity = get_valid_password_reset_token(&pool, "reset-token-abc")
        .await
        .unwrap();
    assert_eq!(token_entity.user_id, user_id.value());
    assert_eq!(token_entity.token, "reset-token-abc");
    assert!(token_entity.used_at.is_none());
}

#[tokio::test]
async fn get_valid_password_reset_token_works() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (validated, password) = validated_create(sample_create_req());
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let expires_at = Utc::now() + Duration::hours(24);
    create_password_reset_token(&mut *conn2, user_id, "valid-reset-token", expires_at)
        .await
        .unwrap();

    let result = get_valid_password_reset_token(&pool, "valid-reset-token").await;
    assert!(result.is_ok());
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

#[tokio::test]
async fn mark_reset_token_used_sets_used_at() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (validated, password) = validated_create(sample_create_req());
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let expires_at = Utc::now() + Duration::hours(24);
    create_password_reset_token(&mut *conn2, user_id, "use-me-reset-token", expires_at)
        .await
        .unwrap();

    let token_entity = get_valid_password_reset_token(&pool, "use-me-reset-token")
        .await
        .unwrap();

    let mut conn3 = pool.acquire().await.unwrap();
    mark_reset_token_used(&mut *conn3, PasswordTokenId::new(token_entity.id))
        .await
        .unwrap();

    // Token should no longer be valid (used_at is set)
    let result = get_valid_password_reset_token(&pool, "use-me-reset-token").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_user_password_changes_hash() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let username = req.username.clone();
    let (validated, password) = validated_create(req);
    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let original_user = get_user_by_username(&pool, Username::new(&username).unwrap())
        .await
        .unwrap();
    let original_password = original_user.password.clone();

    let mut conn2 = pool.acquire().await.unwrap();
    update_user_password(&mut *conn2, user_id, "new-hashed-password")
        .await
        .unwrap();

    let updated_user = get_user_by_id(&pool, user_id).await.unwrap();
    assert_ne!(updated_user.password, original_password);
    assert_eq!(updated_user.password, "new-hashed-password");
    assert!(updated_user.updated_at.is_some());
}
