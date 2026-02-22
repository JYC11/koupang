use sqlx::{PgConnection, Pool, Postgres, Transaction};
use std::ops::DerefMut;
use std::pin::Pin;

// just copied https://github.com/kano1101/sqlx-transaction-manager/tree/main
// and replaced MySql types with Postgres types

#[derive(Debug, thiserror::Error)]
pub enum TxError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Transaction has already been consumed")]
    AlreadyConsumed,
    #[error("{0}")]
    Other(String),
}

pub type TxResult<T> = Result<T, TxError>;

pub struct TxContext<'tx> {
    tx: Option<Transaction<'tx, Postgres>>,
}

impl<'tx> TxContext<'tx> {
    pub async fn begin(pool: &Pool<Postgres>) -> TxResult<Self> {
        Ok(Self {
            tx: Some(pool.begin().await?),
        })
    }

    pub async fn commit(mut self) -> TxResult<()> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await?;
        }
        Ok(())
    }

    pub async fn rollback(mut self) -> TxResult<()> {
        if let Some(tx) = self.tx.take() {
            tx.rollback().await?;
        }
        Ok(())
    }

    pub fn as_executor(&mut self) -> &mut PgConnection {
        self.tx
            .as_mut()
            .expect("Transaction has already been consumed")
            .deref_mut()
    }

    #[allow(dead_code)]
    pub fn into_inner(mut self) -> Transaction<'tx, Postgres> {
        self.tx
            .take()
            .expect("Transaction has already been consumed")
    }
}

impl<'tx> Drop for TxContext<'tx> {
    fn drop(&mut self) {}
}

pub async fn with_transaction<F, T>(pool: &Pool<Postgres>, f: F) -> TxResult<T>
where
    F: for<'a> FnOnce(
        &'a mut TxContext<'_>,
    ) -> Pin<Box<dyn Future<Output = TxResult<T>> + Send + 'a>>,
    T: Send,
{
    let mut tx_ctx = TxContext::begin(pool).await?;

    match f(&mut tx_ctx).await {
        Ok(result) => {
            tx_ctx.commit().await?;
            Ok(result)
        }
        Err(e) => {
            let _ = tx_ctx.rollback().await;
            Err(e)
        }
    }
}

pub async fn with_nested_transaction<F, T>(tx_ctx: &mut TxContext<'_>, f: F) -> TxResult<T>
where
    F: for<'a> FnOnce(
        &'a mut TxContext<'_>,
    ) -> Pin<Box<dyn Future<Output = TxResult<T>> + Send + 'a>>,
    T: Send,
{
    // Create a savepoint
    sqlx::query("SAVEPOINT nested_tx")
        .execute(tx_ctx.as_executor())
        .await?;

    match f(tx_ctx).await {
        Ok(result) => {
            // Release savepoint (equivalent to commit)
            sqlx::query("RELEASE SAVEPOINT nested_tx")
                .execute(tx_ctx.as_executor())
                .await?;
            Ok(result)
        }
        Err(e) => {
            // Rollback to savepoint
            let _ = sqlx::query("ROLLBACK TO SAVEPOINT nested_tx")
                .execute(tx_ctx.as_executor())
                .await;
            Err(e)
        }
    }
}
