//! Owned PostgreSQL schemas for outbox and real relay proofs, never production.

use sqlx::{Connection, PgConnection};
use std::error::Error;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

pub(crate) type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const CONNECTION_DEADLINE: Duration = Duration::from_secs(5);
const SETUP_DEADLINE: Duration = Duration::from_secs(10);
const CASE_DEADLINE: Duration = Duration::from_secs(30);
const CLEANUP_DEADLINE: Duration = Duration::from_secs(10);
static NEXT_SCHEMA: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub(crate) struct Fixture {
    postgres_url: String,
    schema: String,
    owned: Arc<AtomicBool>,
}

impl Fixture {
    pub(crate) fn new() -> TestResult<Self> {
        Ok(Self {
            postgres_url: std::env::var("EDGEAGENT_POSTGRES_URL")?,
            schema: format!(
                "ea_ob_{:x}_{:x}_{:x}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros(),
                NEXT_SCHEMA.fetch_add(1, Ordering::Relaxed),
            ),
            owned: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(crate) async fn connect(&self) -> TestResult<PgConnection> {
        timeout(CONNECTION_DEADLINE, async {
            let mut connection = PgConnection::connect(&self.postgres_url).await?;
            // The generated identifier contains only our prefix, hex digits and
            // underscores. Include session setup inside the connection deadline.
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "SET search_path TO {}; SET default_transaction_isolation = 'read committed'; SET statement_timeout = '5s'; SET lock_timeout = '3s';",
                self.schema,
            )))
            .execute(&mut connection)
            .await?;
            Ok(connection)
        })
        .await?
    }

    async fn initialize(&self) -> TestResult {
        let mut connection = self.connect().await?;
        // DDL stays uncommitted until ownership is recorded. Failed CREATE must
        // never authorize deleting a pre-existing schema with the same name.
        let mut transaction = connection.begin().await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE SCHEMA {};",
            self.schema
        )))
        .execute(&mut *transaction)
        .await?;
        self.owned.store(true, Ordering::Release);
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn exists(&self) -> TestResult<bool> {
        let mut connection = self.connect().await?;
        Ok(
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1)")
                .bind(&self.schema)
                .fetch_one(&mut connection)
                .await?,
        )
    }

    async fn cleanup(&self) -> TestResult {
        if !self.owned.load(Ordering::Acquire) {
            return Ok(());
        }
        timeout(CLEANUP_DEADLINE, async {
            let mut connection = self.connect().await?;
            // Remove only this run's explicitly owned resource, not public or
            // another test's schema. No async recovery is hidden in Drop.
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "SET search_path TO public; DROP SCHEMA IF EXISTS {} CASCADE;",
                self.schema,
            )))
            .execute(&mut connection)
            .await?;
            if self.exists().await? {
                return Err("owned outbox schema survived cleanup".into());
            }
            Ok(())
        })
        .await?
    }

    pub(crate) async fn run<Case, Work>(&self, deadline: Duration, case: Case) -> TestResult
    where
        Case: FnOnce(Self) -> Work + Send + 'static,
        Work: Future<Output = TestResult> + Send + 'static,
    {
        let owned = self.clone();
        // The task catches synchronous closure panics as well as async panics.
        // The inner deadline starts after setup and drops all case-owned work.
        let mut task = tokio::spawn(async move {
            timeout(SETUP_DEADLINE, owned.initialize()).await??;
            timeout(deadline, async move { case(owned).await }).await?
        });
        let result: TestResult =
            match timeout(SETUP_DEADLINE + deadline + CLEANUP_DEADLINE, &mut task).await {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => Err(error.into()),
                Err(error) => {
                    task.abort();
                    // Await resource release before attempting schema deletion.
                    match timeout(CLEANUP_DEADLINE, task).await {
                        Ok(_) => Err(error.into()),
                        Err(shutdown) => Err(format!(
                            "outbox proof timed out ({error}); task shutdown timed out ({shutdown})"
                        )
                        .into()),
                    }
                }
            };
        match (result, self.cleanup().await) {
            (Ok(()), cleanup) => cleanup,
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(cleanup)) => Err(format!(
                "outbox proof failed ({error}); fixture cleanup failed ({cleanup})"
            )
            .into()),
        }
    }
}

pub(crate) async fn with_schema<Case, Work>(case: Case) -> TestResult
where
    Case: FnOnce(Fixture) -> Work + Send + 'static,
    Work: Future<Output = TestResult> + Send + 'static,
{
    Fixture::new()?.run(CASE_DEADLINE, case).await
}
