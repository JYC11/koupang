pub mod pagination_support;
pub mod transaction_support;

use crate::config::db_config::DbConfig;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, Pool, Postgres};
use std::error::Error;

pub trait PgExec<'e>: Executor<'e, Database = Postgres> {}
impl<'e, T: Executor<'e, Database = Postgres>> PgExec<'e> for T {}
pub type PgPool = Pool<Postgres>;

async fn connect_db(db_config: DbConfig) -> Result<Pool<Postgres>, Box<dyn Error>> {
    let pool = PgPoolOptions::new()
        .max_connections(db_config.max_connections)
        .connect(&db_config.url)
        .await?;
    Ok(pool)
}

async fn migrate_db(pool: &Pool<Postgres>, migrations_dir: &str) -> Result<(), Box<dyn Error>> {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let migrations_dir = std::path::Path::new(&crate_dir).join(migrations_dir);
    sqlx::migrate::Migrator::new(migrations_dir)
        .await?
        .run(pool)
        .await?;
    tracing::info!("Database migration completed successfully");
    Ok(())
}

pub async fn init_db(
    db_config: DbConfig,
    migrations_dir: &str,
) -> Result<Pool<Postgres>, Box<dyn Error>> {
    let pool = connect_db(db_config).await?;
    migrate_db(&pool, migrations_dir).await?;
    Ok(pool)
}
