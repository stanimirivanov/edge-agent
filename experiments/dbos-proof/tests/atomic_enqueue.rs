//! Proves PostgreSQL atomicity of service data plus DBOS's SQL enqueue function.
//! This is intentionally not a Rust SDK workflow-execution test.

use std::error::Error;

use testcontainers::{
    GenericImage,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio_postgres::{NoTls, error::SqlState};

// DBOS-maintained PostgreSQL 16 test image with its system schema at migration 108.
// The index digest covers both linux/amd64 and linux/arm64 manifests.
const DBOS_POSTGRES_IMAGE: &str = "ghcr.io/dbos-inc/dbos-test-postgres";
const DBOS_POSTGRES_TAG_AND_DIGEST: &str =
    "16-m108@sha256:7575d6a293d7bf139300deef8b0eee70b15d8ae12dafd725a73a8cfbfc89cb57";

#[tokio::test]
async fn application_row_and_dbos_enqueue_commit_or_rollback_together() -> Result<(), Box<dyn Error>>
{
    let postgres = GenericImage::new(DBOS_POSTGRES_IMAGE, DBOS_POSTGRES_TAG_AND_DIGEST)
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .start()
        .await?;

    let host = postgres.get_host().await?.to_string();
    let port = postgres.get_host_port_ipv4(5432.tcp()).await?;
    let mut config = tokio_postgres::Config::new();
    config
        .host(&host)
        .port(port)
        .user("postgres")
        .password("postgres")
        .dbname("dbos_test_1");
    let (mut client, connection) = config.connect(NoTls).await?;
    let connection_task = tokio::spawn(connection);

    let schema_version: i64 = client
        .query_one("SELECT version::bigint FROM dbos.dbos_migrations", &[])
        .await?
        .get(0);
    assert!(
        schema_version >= 108,
        "DBOS SQL function requires migration 108"
    );
    let function_matches: bool = client
        .query_one(
            "SELECT EXISTS (
                 SELECT 1
                 FROM pg_proc AS p
                 JOIN pg_namespace AS n ON n.oid = p.pronamespace
                 WHERE n.nspname = 'dbos'
                   AND p.proname = 'enqueue_workflow'
                   AND p.prorettype = 'text'::regtype
                   AND p.proargnames @> ARRAY[
                       'workflow_name', 'queue_name', 'positional_args', 'workflow_id'
                   ]::text[]
             )",
            &[],
        )
        .await?
        .get(0);
    assert!(
        function_matches,
        "DBOS schema must provide the documented enqueue_workflow function"
    );

    client
        .batch_execute(
            "CREATE SCHEMA app;
             CREATE TABLE app.evaluation_jobs (
                 evaluation_id TEXT PRIMARY KEY,
                 evidence_ref TEXT NOT NULL
             );",
        )
        .await?;

    let rollback_workflow_id = "m08-dbos-rollback";
    let transaction = client.transaction().await?;
    transaction
        .execute(
            "INSERT INTO app.evaluation_jobs (evaluation_id, evidence_ref) VALUES ($1, $2)",
            &[&"rollback", &"synthetic-evidence-v1"],
        )
        .await?;
    let enqueued_id = enqueue(&transaction, "rollback", rollback_workflow_id).await?;
    assert_eq!(enqueued_id, rollback_workflow_id);
    assert_eq!(
        count_workflows(&transaction, rollback_workflow_id).await?,
        1
    );

    // Force a real PostgreSQL statement error *after* both writes. The failed
    // transaction must roll back its app and DBOS writes together.
    let failure = transaction
        .execute(
            "INSERT INTO app.evaluation_jobs (evaluation_id, evidence_ref) VALUES ($1, $2)",
            &[&"rollback", &"duplicate"],
        )
        .await
        .expect_err("duplicate application key must fail");
    assert_eq!(failure.code(), Some(&SqlState::UNIQUE_VIOLATION));
    transaction.rollback().await?;
    assert_eq!(count_app_rows(&client, "rollback").await?, 0);
    assert_eq!(count_workflows(&client, rollback_workflow_id).await?, 0);

    let commit_workflow_id = "m08-dbos-commit";
    let transaction = client.transaction().await?;
    transaction
        .execute(
            "INSERT INTO app.evaluation_jobs (evaluation_id, evidence_ref) VALUES ($1, $2)",
            &[&"commit", &"synthetic-evidence-v1"],
        )
        .await?;
    let enqueued_id = enqueue(&transaction, "commit", commit_workflow_id).await?;
    assert_eq!(enqueued_id, commit_workflow_id);
    transaction.commit().await?;
    assert_eq!(count_app_rows(&client, "commit").await?, 1);
    assert_eq!(count_workflows(&client, commit_workflow_id).await?, 1);
    let workflow = client
        .query_one(
            "SELECT status, queue_name, serialization
             FROM dbos.workflow_status WHERE workflow_uuid = $1",
            &[&commit_workflow_id],
        )
        .await?;
    let status: String = workflow.get(0);
    let queue_name: Option<String> = workflow.get(1);
    let serialization: Option<String> = workflow.get(2);
    assert_eq!(status, "ENQUEUED");
    assert_eq!(queue_name.as_deref(), Some("m08_evaluations"));
    assert_eq!(serialization.as_deref(), Some("portable_json"));

    connection_task.abort();
    Ok(())
}

async fn enqueue(
    transaction: &tokio_postgres::Transaction<'_>,
    evaluation_id: &str,
    workflow_id: &str,
) -> Result<String, tokio_postgres::Error> {
    let row = transaction
        .query_one(
            "SELECT dbos.enqueue_workflow(
                workflow_name => 'm08_synthetic',
                queue_name => 'm08_evaluations',
                positional_args => ARRAY[to_json($1::text)],
                workflow_id => $2::text
            )",
            &[&evaluation_id, &workflow_id],
        )
        .await?;
    Ok(row.get(0))
}

async fn count_app_rows(
    client: &impl tokio_postgres::GenericClient,
    evaluation_id: &str,
) -> Result<i64, tokio_postgres::Error> {
    let row = client
        .query_one(
            "SELECT count(*) FROM app.evaluation_jobs WHERE evaluation_id = $1",
            &[&evaluation_id],
        )
        .await?;
    Ok(row.get(0))
}

async fn count_workflows(
    client: &impl tokio_postgres::GenericClient,
    workflow_id: &str,
) -> Result<i64, tokio_postgres::Error> {
    let row = client
        .query_one(
            "SELECT count(*) FROM dbos.workflow_status WHERE workflow_uuid = $1",
            &[&workflow_id],
        )
        .await?;
    Ok(row.get(0))
}
