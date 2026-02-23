use crate::common::{sample_create_req, sample_create_req_2, sample_update_req};
use identity::users::repository::*;
use shared::db::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = "./migrations")]
async fn create_user_inserts_row(pool: PgPool) {
    let req = sample_create_req();
    let username = req.username.clone();
    let email = req.email.clone();
    let phone = req.phone.clone();
    let role = req.role.clone();

    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, req).await.unwrap();

    let user = get_user_by_username(&pool, &username).await.unwrap();
    assert_eq!(user.username, username);
    assert_eq!(user.email, email);
    assert_eq!(user.phone, phone);
    assert_eq!(user.role, role);
    assert!(user.deleted_at.is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn create_user_duplicate_username_fails(pool: PgPool) {
    let req1 = sample_create_req();
    let mut req2 = sample_create_req_2();
    req2.username = req1.username.clone();

    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, req1).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let result = create_user(&mut *conn2, req2).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn create_user_duplicate_email_fails(pool: PgPool) {
    let req1 = sample_create_req();
    let mut req2 = sample_create_req_2();
    req2.email = req1.email.clone();

    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, req1).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    let result = create_user(&mut *conn2, req2).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_by_id_returns_existing(pool: PgPool) {
    let req = sample_create_req();
    let username = req.username.clone();

    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, req).await.unwrap();

    let created = get_user_by_username(&pool, &username).await.unwrap();
    let fetched = get_user_by_id(&pool, created.id).await.unwrap();

    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.username, created.username);
    assert_eq!(fetched.email, created.email);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_by_id_nonexistent_returns_error(pool: PgPool) {
    let result = get_user_by_id(&pool, Uuid::new_v4()).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_by_username_returns_existing(pool: PgPool) {
    let req = sample_create_req();
    let username = req.username.clone();

    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, req).await.unwrap();

    let user = get_user_by_username(&pool, &username).await.unwrap();
    assert_eq!(user.username, username);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_by_username_nonexistent_returns_error(pool: PgPool) {
    let result = get_user_by_username(&pool, &"nonexistent".to_string()).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn update_user_modifies_fields(pool: PgPool) {
    let req = sample_create_req();
    let username = req.username.clone();

    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, req).await.unwrap();

    let created = get_user_by_username(&pool, &username).await.unwrap();
    let update_req = sample_update_req();
    let new_username = update_req.username.clone();
    let new_email = update_req.email.clone();
    let new_phone = update_req.phone.clone();

    let mut conn2 = pool.acquire().await.unwrap();
    update_user(&mut *conn2, created.id, update_req)
        .await
        .unwrap();

    let updated = get_user_by_id(&pool, created.id).await.unwrap();
    assert_eq!(updated.username, new_username);
    assert_eq!(updated.email, new_email);
    assert_eq!(updated.phone, new_phone);
    assert!(updated.updated_at.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn update_nonexistent_user_returns_error(pool: PgPool) {
    let update_req = sample_update_req();

    let mut conn = pool.acquire().await.unwrap();
    let result = update_user(&mut *conn, Uuid::new_v4(), update_req).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_user_soft_deletes(pool: PgPool) {
    let req = sample_create_req();
    let username = req.username.clone();

    let mut conn = pool.acquire().await.unwrap();
    create_user(&mut *conn, req).await.unwrap();

    let created = get_user_by_username(&pool, &username).await.unwrap();

    let mut conn2 = pool.acquire().await.unwrap();
    delete_user(&mut *conn2, created.id).await.unwrap();

    // get_user_by_id filters deleted_at IS NULL, so should return NotFound
    let result = get_user_by_id(&pool, created.id).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_nonexistent_user_returns_error(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap();
    let result = delete_user(&mut *conn, Uuid::new_v4()).await;
    assert!(result.is_err());
}
