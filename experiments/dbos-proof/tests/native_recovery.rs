//! Native DBOS Rust SDK checkpoint recovery, isolated from the SQL enqueue proof.

use std::{error::Error, time::Duration};

use testcontainers::{
    GenericImage,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::{process::Command, time::timeout};
use tokio_postgres::NoTls;

const DBOS_POSTGRES_IMAGE: &str = "ghcr.io/dbos-inc/dbos-test-postgres";
const DBOS_POSTGRES_TAG_AND_DIGEST: &str =
    "16-m108@sha256:7575d6a293d7bf139300deef8b0eee70b15d8ae12dafd725a73a8cfbfc89cb57";

#[tokio::test]
async fn completed_step_is_not_reexecuted_after_worker_process_exit() -> Result<(), Box<dyn Error>>
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
    let url = format!("postgresql://postgres:postgres@{host}:{port}/dbos_test_1");
    let (client, connection) = tokio_postgres::connect(&url, NoTls).await?;
    let connection_task = tokio::spawn(connection);
    client
        .batch_execute(
            "CREATE SCHEMA app;
             CREATE TABLE app.m08_step_counts (
                 step TEXT PRIMARY KEY,
                 runs INTEGER NOT NULL
             );",
        )
        .await?;

    let workflow_id = "m08-native-crash-restart";
    let first = run_worker(&url, workflow_id, "first").await?;
    assert!(
        !first.status.success(),
        "first worker did not crash: {first:?}"
    );
    assert_eq!(runs(&client, "a").await?, 1);
    assert_eq!(runs(&client, "b").await?, 1);

    let resumed = run_worker(&url, workflow_id, "resume").await?;
    assert!(resumed.status.success(), "resumed worker: {resumed:?}");
    assert_eq!(
        runs(&client, "a").await?,
        1,
        "checkpointed A must not rerun"
    );
    assert_eq!(
        runs(&client, "b").await?,
        2,
        "crashed in-flight B must retry"
    );
    let row = client
        .query_one(
            "SELECT status, serialization FROM dbos.workflow_status WHERE workflow_uuid = $1",
            &[&workflow_id],
        )
        .await?;
    let status: String = row.get(0);
    let serialization: Option<String> = row.get(1);
    assert_eq!(status, "SUCCESS");
    assert_eq!(serialization.as_deref(), Some("rust_serde"));
    connection_task.abort();
    Ok(())
}

async fn run_worker(
    url: &str,
    workflow_id: &str,
    phase: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dbos_worker"));
    command
        .env("DBOS_PROOF_DATABASE_URL", url)
        .env("DBOS_PROOF_WORKFLOW_ID", workflow_id)
        .env("DBOS_PROOF_PHASE", phase)
        .kill_on_drop(true);
    Ok(timeout(Duration::from_secs(90), command.output()).await??)
}

async fn runs(client: &tokio_postgres::Client, step: &str) -> Result<i32, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT runs FROM app.m08_step_counts WHERE step = $1",
            &[&step],
        )
        .await?
        .get(0))
}
