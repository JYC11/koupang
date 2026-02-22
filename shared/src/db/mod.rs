pub mod pagination_support;
pub mod transaction_support;

use crate::config::db_config::DbConfig;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, Pool, Postgres, Transaction};

pub type PgTx<'a> = Transaction<'a, Postgres>;
pub trait PgExec<'e>: Executor<'e, Database = Postgres> {}
impl<'e, T: Executor<'e, Database = Postgres>> PgExec<'e> for T {}
pub type PgPool = Pool<Postgres>;

async fn connect_db(db_config: DbConfig) -> Pool<Postgres> {
    PgPoolOptions::new()
        .max_connections(db_config.max_connections)
        .connect(&*db_config.url)
        .await
        .unwrap()
}

async fn migrate_db(pool: &Pool<Postgres>, migrations_dir: &str) {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let migrations_dir = std::path::Path::new(&crate_dir).join(migrations_dir);
    let migration_results = sqlx::migrate::Migrator::new(migrations_dir)
        .await
        .unwrap()
        .run(pool)
        .await;
    match migration_results {
        Ok(_) => println!("Migration success"),
        Err(error) => {
            panic!("error: {}", error);
        }
    }
    println!("migration: {:?}", migration_results);
}

pub async fn init_db(db_config: DbConfig, migrations_dir: &str) -> Pool<Postgres> {
    let pool = connect_db(db_config).await;
    migrate_db(&pool, migrations_dir).await;
    pool
}
