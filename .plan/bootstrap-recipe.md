# New Service Bootstrap Recipe

Use catalog as the reference implementation:

1. **Add crate to workspace** `Cargo.toml` members list
2. **Create `src/main.rs`** — use `run_service_with_infra()`:
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
           None::<NoGrpc>,  // or Some((GrpcConfig { .. }, grpc_router)) for gRPC
           |pool, redis_conn| {
               let app_state = AppState::new(pool, redis_conn);
               app(app_state).merge(health_routes("<service>"))
           },
       ).await
   }
   ```
3. **Create `src/lib.rs`** — define `AppState` (wraps `Arc<Service>` + `Arc<JwtService>`) and `app()` fn
4. **Create module directory** e.g. `src/orders/` with: `mod.rs`, `routes.rs`, `service.rs`, `domain.rs`, `repository.rs`, `entities.rs`, `dtos.rs`, `value_objects.rs`
5. **Create first migration**: `make migration SERVICE=<name> NAME=init`
6. **Auth**: use `AuthMiddleware::new_claims_based(jwt_service)` for non-identity services (ADR-008)
7. **Tests**: create `tests/integration.rs`, `tests/common/mod.rs` (with `test_db()`, `test_app_state()`, sample fixtures), and per-module test files
8. **Add CLAUDE.md** in the service directory
9. **Add env vars** to `docker-compose.yml`
