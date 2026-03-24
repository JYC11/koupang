use payment::gateway::mock::MockPaymentGateway;
use shared::db::PgPool;
use shared::events::{EventEnvelope, EventType};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::ImageExt;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use tokio::sync::OnceCell;

/// Shared Postgres container for saga tests, with 3 template databases (one per service).
struct SagaContainer {
    _container: ContainerAsync<Postgres>,
    connection_base: String,
    db_counter: AtomicU32,
}

static SAGA_PG: OnceCell<SagaContainer> = OnceCell::const_new();

impl SagaContainer {
    async fn init() -> Self {
        let container = Postgres::default()
            .with_tag("18.0-alpine3.21")
            .start()
            .await
            .unwrap();

        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let connection_base = format!("postgres://postgres:postgres@{host}:{port}");

        let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

        // Create 3 template databases, one per service.
        for (template_name, migrations_dir) in [
            ("order_template", "../order/migrations"),
            ("catalog_template", "../catalog/migrations"),
            ("payment_template", "../payment/migrations"),
        ] {
            let admin_pool = PgPoolOptions::new()
                .max_connections(2)
                .connect(&format!("{connection_base}/postgres"))
                .await
                .unwrap();

            sqlx::query(&format!("CREATE DATABASE {template_name}"))
                .execute(&admin_pool)
                .await
                .unwrap();

            admin_pool.close().await;

            let template_pool = PgPoolOptions::new()
                .max_connections(2)
                .connect(&format!("{connection_base}/{template_name}"))
                .await
                .unwrap();

            let migrations_path = std::path::Path::new(&crate_dir).join(migrations_dir);
            sqlx::migrate::Migrator::new(migrations_path)
                .await
                .unwrap()
                .run(&template_pool)
                .await
                .unwrap();

            template_pool.close().await;
        }

        Self {
            _container: container,
            connection_base,
            db_counter: AtomicU32::new(0),
        }
    }

    async fn create_db(&self, template_name: &str) -> PgPool {
        let n = self.db_counter.fetch_add(1, Ordering::Relaxed);
        let db_name = format!("saga_db_{n}");

        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&format!("{}/postgres", self.connection_base))
            .await
            .unwrap();

        sqlx::query(&format!(
            "CREATE DATABASE {db_name} TEMPLATE {template_name}"
        ))
        .execute(&admin_pool)
        .await
        .unwrap();

        admin_pool.close().await;

        PgPoolOptions::new()
            .max_connections(5)
            .connect(&format!("{}/{db_name}", self.connection_base))
            .await
            .unwrap()
    }
}

/// Three-service test harness for saga event-threading simulation.
///
/// Each service gets its own database (shared Postgres container, separate DBs).
/// Events are threaded manually: drain outbox → deliver to next handler.
pub struct SagaHarness {
    order_pool: PgPool,
    catalog_pool: PgPool,
    payment_pool: PgPool,
    pub gateway: Arc<MockPaymentGateway>,
}

impl SagaHarness {
    pub async fn start(gateway: MockPaymentGateway) -> Self {
        let shared = SAGA_PG.get_or_init(SagaContainer::init).await;

        let order_pool = shared.create_db("order_template").await;
        let catalog_pool = shared.create_db("catalog_template").await;
        let payment_pool = shared.create_db("payment_template").await;

        Self {
            order_pool,
            catalog_pool,
            payment_pool,
            gateway: Arc::new(gateway),
        }
    }

    pub fn order_pool(&self) -> &PgPool {
        &self.order_pool
    }

    pub fn catalog_pool(&self) -> &PgPool {
        &self.catalog_pool
    }

    pub fn payment_pool(&self) -> &PgPool {
        &self.payment_pool
    }

    // ── Outbox operations ──────────────────────────────────────

    /// Drain the next pending outbox event of the given type, mark it as published,
    /// and return the reconstructed EventEnvelope.
    pub async fn drain_outbox(&self, pool: &PgPool, event_type: &str) -> EventEnvelope {
        let row: Option<OutboxRow> = sqlx::query_as(
            "UPDATE outbox_events SET status = 'published', published_at = NOW()
             WHERE id = (
                 SELECT id FROM outbox_events
                 WHERE event_type = $1 AND status = 'pending'
                 ORDER BY created_at ASC
                 LIMIT 1
             )
             RETURNING payload",
        )
        .bind(event_type)
        .fetch_optional(pool)
        .await
        .unwrap_or_else(|e| panic!("Failed to drain outbox for {event_type}: {e}"));

        let row =
            row.unwrap_or_else(|| panic!("No pending outbox event of type '{event_type}' found"));

        row.into_envelope()
    }

    /// Assert no pending outbox events remain in this pool.
    pub async fn assert_outbox_drained(&self, pool: &PgPool, label: &str) {
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE status = 'pending'")
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(
            count.0,
            0,
            "{label} DB still has {count} pending outbox events",
            label = label,
            count = count.0,
        );
    }

    /// Assert all three service outboxes are drained.
    pub async fn assert_all_outboxes_drained(&self) {
        self.assert_outbox_drained(self.order_pool(), "order").await;
        self.assert_outbox_drained(self.catalog_pool(), "catalog")
            .await;
        self.assert_outbox_drained(self.payment_pool(), "payment")
            .await;
    }

    // ── Deliver to service handlers ─────────────────────────────

    /// Deliver an envelope to the order service's consumer handlers.
    pub async fn deliver_to_order(&self, envelope: &EventEnvelope) {
        let mut conn = self.order_pool().acquire().await.unwrap();
        match &envelope.metadata.event_type {
            EventType::InventoryReserved => {
                order::consumers::inventory_events::handle_inventory_reserved(&mut *conn, envelope)
                    .await
                    .unwrap();
            }
            EventType::InventoryReservationFailed => {
                order::consumers::inventory_events::handle_inventory_reservation_failed(
                    &mut *conn, envelope,
                )
                .await
                .unwrap();
            }
            EventType::PaymentAuthorized => {
                order::consumers::payment_events::handle_payment_authorized(&mut *conn, envelope)
                    .await
                    .unwrap();
            }
            EventType::PaymentFailed => {
                order::consumers::payment_events::handle_payment_failed(&mut *conn, envelope)
                    .await
                    .unwrap();
            }
            EventType::PaymentTimedOut => {
                order::consumers::payment_events::handle_payment_timed_out(&mut *conn, envelope)
                    .await
                    .unwrap();
            }
            other => panic!("Order service does not handle {other:?}"),
        }
    }

    /// Deliver an envelope to the catalog service's consumer handlers.
    /// Returns Ok(()) on success, Err(msg) if the handler returned a permanent error
    /// (e.g. inventory reservation failed — but InventoryReservationFailed was still written on pool).
    pub async fn deliver_to_catalog(&self, envelope: &EventEnvelope) -> Result<(), String> {
        let pool = self.catalog_pool();
        let mut conn = pool.acquire().await.unwrap();
        match &envelope.metadata.event_type {
            EventType::OrderCreated => {
                catalog::consumers::order_events::handle_order_created(&mut *conn, pool, envelope)
                    .await
                    .map_err(|e| e.to_string())
            }
            EventType::OrderCancelled => {
                catalog::consumers::order_events::handle_order_cancelled(&mut *conn, envelope)
                    .await
                    .map_err(|e| e.to_string())
            }
            other => panic!("Catalog service does not handle {other:?}"),
        }
    }

    /// Deliver an envelope to the payment service's consumer handlers.
    pub async fn deliver_to_payment(&self, envelope: &EventEnvelope) {
        let pool = self.payment_pool();
        let mut conn = pool.acquire().await.unwrap();
        match &envelope.metadata.event_type {
            EventType::InventoryReserved => {
                payment::consumers::inventory_events::handle_inventory_reserved(
                    &mut *conn,
                    pool,
                    self.gateway.as_ref(),
                    envelope,
                )
                .await
                .unwrap();
            }
            EventType::OrderConfirmed => {
                payment::consumers::order_events::handle_order_confirmed(
                    &mut *conn,
                    pool,
                    self.gateway.as_ref(),
                    envelope,
                )
                .await
                .unwrap();
            }
            EventType::OrderCancelled => {
                payment::consumers::order_events::handle_order_cancelled(
                    &mut *conn,
                    pool,
                    self.gateway.as_ref(),
                    envelope,
                )
                .await
                .unwrap();
            }
            other => panic!("Payment service does not handle {other:?}"),
        }
    }
}

// ── OutboxRow → EventEnvelope reconstruction ──────────────────

/// The outbox `payload` column stores the full serialized EventEnvelope
/// (metadata + payload), not just the domain payload. We deserialize it directly.
#[derive(sqlx::FromRow)]
struct OutboxRow {
    payload: serde_json::Value,
}

impl OutboxRow {
    fn into_envelope(self) -> EventEnvelope {
        serde_json::from_value(self.payload)
            .expect("outbox payload should deserialize as EventEnvelope")
    }
}
