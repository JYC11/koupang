---
name: implement
description: >
  Implement a new endpoint or module following the project's layered architecture. Use when the
  user says "add endpoint", "new module", "new route", "implement feature", "add API", "create
  handler", or is building routes/service/domain/repository layers for an existing service.
---

# Implement an Endpoint or Module

Follow the layered architecture: routes -> service (free fns) -> domain -> repository (free fns).

## File Structure per Module

```
src/<module>/
├── mod.rs            # re-exports (domain module uses #[allow(dead_code)] if unused yet)
├── routes.rs         # HTTP handlers
├── service.rs        # orchestration via free functions (not structs)
├── domain.rs         # rich domain objects with value objects
├── repository.rs     # pure SQL via free functions (PgConnection for writes, PgExec for reads)
├── entities.rs       # raw DB rows (String, Decimal, Uuid) — sqlx FromRow
├── dtos.rs           # request/response DTOs + validated request types
└── value_objects.rs  # newtype wrappers enforcing invariants
```

## Layer Pattern

```rust
// routes.rs — extract state + auth + body, validate at boundary, delegate to service
async fn create_thing(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(body): Json<CreateThingReq>,
) -> Result<impl IntoResponse, AppError> {
    let thing = service::create_thing(&state, &current_user, body).await?;
    Ok(responses::ok(thing))
}

// service.rs — free functions, orchestration only
pub async fn create_thing(
    state: &AppState,
    user: &CurrentUser,
    req: CreateThingReq,
) -> Result<ThingRes, AppError> {
    let validated = ValidCreateThingReq::new(req)?;  // VO construction only
    repository::validate_fk_references(&state.pool, validated.category_id).await?;  // FK checks in service
    let thing_id = with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::create_thing(tx.as_executor(), user.id, validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    }).await?;
    let entity = repository::get_thing_by_id(&state.pool, thing_id).await?;
    Ok(ThingRes::new(entity))
}

// repository.rs — free functions, pure SQL; use QueryBuilder::separated() for partial updates
pub async fn create_thing(tx: &mut PgConnection, user_id: Uuid, req: ValidCreateThingReq) -> Result<ThingId, AppError> {
    let row: (Uuid,) = sqlx::query_as("INSERT INTO things (...) VALUES (...) RETURNING id")
        .bind(...)
        .fetch_one(tx).await
        .map_err(|e| AppError::InternalServerError(format!("Failed to create thing: {}", e)))?;
    Ok(ThingId::new(row.0))
}
```

## Key Conventions

- **Partial updates:** Use `QueryBuilder::separated(", ")` — single if-chain per field, no dual-if-chain
- **Paginated lists:** Use `PaginationQuery` in route, `keyset_paginate()` in repo, return `PaginatedResponse<T>`
- **Bounded queries:** All list queries need LIMIT (keyset pagination or hard cap)
- **Assertions:** `assert_eq!(result.rows_affected(), 1)` after single-row writes
- **Existence checks:** Use `SELECT EXISTS(...)` not `SELECT COUNT(*)`
- **Search strings:** Wrap in `SearchQuery` VO (max 200 chars)
- **Password handling:** `Password` for raw input validation, `HashedPassword` for stored hashes

## Data Flow

```
entities.rs   → Raw DB rows (String, Decimal, Uuid) — used by sqlx FromRow
domain.rs     → Rich domain objects (ProductName, Price) — business invariants via debug_assert!
dtos.rs       → Request/response DTOs + validated request types (VO construction only, no DB)
value_objects.rs → Newtype wrappers enforcing invariants (non-empty, range, format)
```

- **Entity -> Domain**: `TryFrom<Entity>` lifts primitives into value objects; `debug_assert!` on DB-side invariants
- **Validated request types** enforce value object rules only; FK validation happens in the service layer
