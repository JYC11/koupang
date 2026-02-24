# Common Code Patterns

Reference doc for the project's layered architecture patterns. Read on-demand when implementing new endpoints/modules.

## Adding an endpoint (route → service → domain → repository)

```rust
// routes.rs — HTTP handler, extracts state + auth + body
async fn create_thing(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(body): Json<CreateThingReq>,
) -> Result<impl IntoResponse, AppError> {
    let thing = state.service.create_thing(&current_user, body).await?;
    Ok(created("Thing created"))
}

// service.rs — orchestration only (no business logic)
pub async fn create_thing(&self, user: &CurrentUser, req: CreateThingReq) -> Result<ThingDto, AppError> {
    let vo_validated = ValidCreateThingReq::try_from(req)?;         // Step 1: VO validation (dtos.rs)
    let validated = ValidatedCreateThing::new(&self.pool, vo_validated).await?;  // Step 2: domain validation (dtos.rs)
    let entity = with_transaction(&self.pool, |tx| async move {
        repository::create_thing(tx.as_executor(), &validated, user.id).await
    }).await?;
    Ok(ThingDto::from(entity))
}

// domain.rs — rich domain model objects (all fields are value objects)
pub struct Thing {
    pub id: Uuid,
    pub name: ThingName,       // not String — value object with invariants
    pub price: Price,          // not Decimal — value object enforcing >= 0
    pub category_id: Option<Uuid>,  // future: Option<Category> for traversal
}
impl TryFrom<ThingEntity> for Thing { /* lift raw DB row into rich domain model */ }

// repository.rs — pure SQL, takes PgConnection for writes, PgExec for reads
pub async fn create_thing(conn: &mut PgConnection, req: &ValidatedCreateThing, user_id: Uuid) -> Result<ThingEntity, AppError> {
    sqlx::query_as!(ThingEntity, "INSERT INTO things ...")
        .fetch_one(conn).await
        .map_err(|e| AppError::InternalServerError(e.to_string()))
}
```

## Domain layer pattern

Each module has a `domain.rs` with rich domain model objects:

```
entities.rs   → Raw DB rows (String, Decimal, Uuid) — used by sqlx FromRow
domain.rs     → Rich domain objects (ProductName, Price, Currency) — business logic lives here
dtos.rs       → Request/response DTOs + validated request types (VO validation + FK validation)
value_objects.rs → Newtype wrappers enforcing invariants (non-empty, range, format)
```

- **Entity → Domain**: `TryFrom<Entity>` lifts primitives into value objects
- **Domain objects** carry all invariants via the type system; business rules are methods on these types
- **FK references** are currently `Option<Uuid>` — will evolve to embedded domain objects for traversable graphs
- **Validated request types** (e.g. `ValidatedCreateProduct`) enforce FK existence + cross-entity rules before reaching the repository

## Writing an integration test

```rust
// tests/common/mod.rs
pub async fn test_db() -> TestDb {
    TestDb::start("./migrations").await
}
pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_jwt(pool, test_auth_config())
}
pub fn seller_user() -> CurrentUser {
    CurrentUser { id: Uuid::new_v4(), role: Role::Seller }
}

// tests/<module>/<layer>_test.rs
#[tokio::test]
async fn test_create_thing() {
    let db = test_db().await;
    let service = test_catalog_service(db.pool.clone());
    let user = seller_user();
    let result = service.create_thing(&user, sample_req()).await;
    assert!(result.is_ok());
}
```
