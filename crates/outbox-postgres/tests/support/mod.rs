//! Current-schema outbox fixtures; the relay upgrade proof uses schema directly.

mod schema;
use edgeagent_outbox_postgres::PostgresOutbox;
pub(super) use schema::{Fixture, TestResult};
use std::future::Future;

pub(super) async fn with_fixture<Case, Work>(case: Case) -> TestResult
where
    Case: FnOnce(Fixture) -> Work + Send + 'static,
    Work: Future<Output = TestResult> + Send + 'static,
{
    schema::with_schema(|fixture| async move {
        let mut connection = fixture.connect().await?;
        for migration in PostgresOutbox::MIGRATIONS {
            sqlx::raw_sql(migration).execute(&mut connection).await?;
        }
        drop(connection);
        case(fixture).await
    })
    .await
}
