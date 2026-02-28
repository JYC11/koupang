---
name: implement
description: >
  Implement a new endpoint or module following the project's layered architecture. Use when the
  user says "add endpoint", "new module", "new route", "implement feature", "add API", "create
  handler", or is building routes/service/domain/repository layers for an existing service.
---

# Implement an Endpoint or Module

Follow the layered architecture: routes -> service -> domain -> repository.

## File Structure per Module

```
src/<module>/
├── mod.rs            # re-exports
├── routes.rs         # HTTP handlers
├── service.rs        # orchestration (no business logic)
├── domain.rs         # rich domain objects with value objects
├── repository.rs     # pure SQL (PgConnection for writes, PgExec for reads)
├── entities.rs       # raw DB rows (String, Decimal, Uuid) — sqlx FromRow
├── dtos.rs           # request/response DTOs + validated request types
└── value_objects.rs  # newtype wrappers enforcing invariants
```

## Layer Pattern

```rust
// routes.rs — extract state + auth + body, delegate to service
async fn create_thing(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(body): Json<CreateThingReq>,
) -> Result<impl IntoResponse, AppError> {
    let thing = state.service.create_thing(&current_user, body).await?;
    Ok(created("Thing created"))
}

// service.rs — orchestration only
pub async fn create_thing(&self, user: &CurrentUser, req: CreateThingReq) -> Result<ThingDto, AppError> {
    let vo_validated = ValidCreateThingReq::try_from(req)?;
    let validated = ValidatedCreateThing::new(&self.pool, vo_validated).await?;
    let entity = with_transaction(&self.pool, |tx| async move {
        repository::create_thing(tx.as_executor(), &validated, user.id).await
    }).await?;
    Ok(ThingDto::from(entity))
}

// domain.rs — all fields are value objects, not primitives
pub struct Thing {
    pub id: Uuid,
    pub name: ThingName,   // not String
    pub price: Price,      // not Decimal
}
impl TryFrom<ThingEntity> for Thing { /* lift DB row into domain model */ }

// repository.rs — pure SQL
pub async fn create_thing(conn: &mut PgConnection, req: &ValidatedCreateThing, user_id: Uuid) -> Result<ThingEntity, AppError> {
    sqlx::query_as!(ThingEntity, "INSERT INTO things ...")
        .fetch_one(conn).await
        .map_err(|e| AppError::InternalServerError(e.to_string()))
}
```

## Data Flow

```
entities.rs   → Raw DB rows (String, Decimal, Uuid) — used by sqlx FromRow
domain.rs     → Rich domain objects (ProductName, Price) — business logic here
dtos.rs       → Request/response DTOs + validated request types (VO + FK validation)
value_objects.rs → Newtype wrappers enforcing invariants (non-empty, range, format)
```

- **Entity -> Domain**: `TryFrom<Entity>` lifts primitives into value objects
- **Validated request types** (e.g. `ValidatedCreateProduct`) enforce FK existence + cross-entity rules before reaching the repository
