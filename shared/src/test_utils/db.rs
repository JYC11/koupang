use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::postgres::PgPoolOptions;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::ImageExt;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use tokio::sync::OnceCell;

/// Shared Postgres container, initialized once per test binary.
/// Each test gets its own database created from a pre-migrated template,
/// eliminating the ~3-8s container startup cost per test.
///
/// NOTE: No pools are stored here. Each `#[tokio::test]` creates its own
/// tokio runtime, so pools from one test's runtime cannot be reused in another.
/// Only the connection URL and container handle are shared.
struct SharedPgContainer {
    _container: ContainerAsync<Postgres>,
    connection_base: String,
    template_db: String,
    db_counter: AtomicU32,
}

static SHARED_PG: OnceCell<SharedPgContainer> = OnceCell::const_new();

impl SharedPgContainer {
    async fn init(migrations_dir: &str) -> Self {
        let container = Postgres::default()
            .with_tag("18.0-alpine3.21")
            .start()
            .await
            .unwrap();

        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let connection_base = format!("postgres://postgres:postgres@{host}:{port}");

        // Create template database
        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&format!("{connection_base}/postgres"))
            .await
            .unwrap();

        let template_db = "test_template".to_string();
        sqlx::query(&format!("CREATE DATABASE {template_db}"))
            .execute(&admin_pool)
            .await
            .unwrap();

        admin_pool.close().await;

        // Connect to template, run migrations, then disconnect
        let template_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&format!("{connection_base}/{template_db}"))
            .await
            .unwrap();

        let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let migrations_path = std::path::Path::new(&crate_dir).join(migrations_dir);
        sqlx::migrate::Migrator::new(migrations_path)
            .await
            .unwrap()
            .run(&template_pool)
            .await
            .unwrap();

        // Disconnect so Postgres allows using this as a TEMPLATE source
        template_pool.close().await;

        Self {
            _container: container,
            connection_base,
            template_db,
            db_counter: AtomicU32::new(0),
        }
    }

    async fn create_test_db(&self) -> TestDb {
        let n = self.db_counter.fetch_add(1, Ordering::Relaxed);
        let db_name = format!("test_db_{n}");

        // Fresh connection in the CURRENT test's tokio runtime
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&format!("{}/postgres", self.connection_base))
            .await
            .unwrap();

        sqlx::query(&format!(
            "CREATE DATABASE {db_name} TEMPLATE {}",
            self.template_db
        ))
        .execute(&admin_pool)
        .await
        .unwrap();

        admin_pool.close().await;

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&format!("{}/{db_name}", self.connection_base))
            .await
            .unwrap();

        TestDb { pool }
    }
}

pub struct TestDb {
    pub pool: crate::db::PgPool,
}

impl TestDb {
    /// Creates an isolated test database from a shared, pre-migrated template.
    ///
    /// The first call starts a single Postgres container and runs migrations once.
    /// Subsequent calls create new databases via `CREATE DATABASE ... TEMPLATE ...`,
    /// which is a file-level copy (~50-100ms vs ~3-8s for a new container).
    pub async fn start(migrations_dir: &str) -> Self {
        let shared = SHARED_PG
            .get_or_init(|| SharedPgContainer::init(migrations_dir))
            .await;
        shared.create_test_db().await
    }
}
