---
name: bootstrap
description: >
  Scaffold a new microservice in the koupang workspace. Use when the user says "new service",
  "scaffold", "bootstrap", "create service", "add service", or starts implementing a stub service
  (order, payment, shipping, notification, review, moderation, bff-gateway).
---

# Bootstrap a New Service

Use catalog as the reference implementation. Execute these steps in order.

## Steps

1. **Add crate to workspace** `Cargo.toml` members list

2. **Create `src/main.rs`**
   ```rust
   use <service>::AppState;
   use <service>::app;
   use shared::health::health_routes;
   use shared::server::{NoGrpc, ServiceConfig, run_service_with_infra};

   #[tokio::main]
   async fn main() -> Result<(), Box<dyn Error>> {
       run_service_with_infra(
           ServiceConfig {
               name: "<service>",
               port_env_key: "<SERVICE>_PORT",
               db_url_env_key: "<SERVICE>_DB_URL",
               migrations_dir: "./.migrations/<service>",
           },
           None::<NoGrpc>,
           |pool, redis_conn| {
               let app_state = AppState::new(pool, redis_conn);
               app(app_state).merge(health_routes("<service>"))
           },
       ).await
   }
   ```

3. **Create `src/lib.rs`** — define `AppState` (wraps `Arc<Service>` + `Arc<JwtService>`) and `app()` fn

4. **Create module directory** e.g. `src/orders/` with: `mod.rs`, `routes.rs`, `service.rs`, `domain.rs`, `repository.rs`, `entities.rs`, `dtos.rs`, `value_objects.rs`, `errors.rs`

5. **Create `errors.rs`** — per-service domain error enum with `From` impl for AppError:
   ```rust
   use shared::errors::AppError;

   #[derive(Debug, thiserror::Error)]
   pub enum OrderError {
       // Domain-specific errors — add variants as business rules emerge
       #[error("order {0} not found")]
       NotFound(uuid::Uuid),

       // Infrastructure passthrough
       #[error(transparent)]
       Infra(#[from] AppError),
   }

   impl From<OrderError> for AppError {
       fn from(e: OrderError) -> Self {
           match e {
               OrderError::NotFound(_) => AppError::NotFound(e.to_string()),
               OrderError::Infra(e) => e,
           }
       }
   }
   ```
   Service/domain layers return `Result<T, OrderError>`. Routes convert via `?` + `From` impl.

6. **Create first migration**: `make migration SERVICE=<name> NAME=init`

7. **Auth**: use `AuthMiddleware::new_claims_based(jwt_service)` for non-identity services (ADR-008)

8. **Kafka topics** (if service publishes events): create topics on startup using `KafkaAdmin::ensure_topics()`, add outbox migration from `.plan/outbox-migration-template.sql`. Follow event naming from ADR-011: `{service}.{entity}.{past_tense_verb}`

9. **Inter-service communication** (ADR-010): REST (`reqwest`) for querying other services' data; Kafka events for notifying state changes. Never share databases between services.

10. **Tests**: create `tests/integration.rs`, `tests/common/mod.rs` (with `test_db()`, `test_app_state()`, sample fixtures), and per-module test files. For event-publishing services, add outbox+kafka integration tests using `TestKafka` and `TestConsumer`

11. **Add CLAUDE.md** in the service directory

12. **Add env vars** to `docker-compose.yml`

13. **Run `make test SERVICE=<name>`** and verify everything compiles
