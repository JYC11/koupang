use sqlx::postgres::PgPoolOptions;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::ImageExt;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

pub struct TestDb {
    pub pool: crate::db::PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestDb {
    pub async fn start(migrations_dir: &str) -> Self {
        let container = Postgres::default()
            .with_tag("18.0-alpine3.21")
            .start()
            .await
            .unwrap();

        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .unwrap();

        let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let migrations_path = std::path::Path::new(&crate_dir).join(migrations_dir);
        sqlx::migrate::Migrator::new(migrations_path)
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();

        Self {
            pool,
            _container: container,
        }
    }
}
