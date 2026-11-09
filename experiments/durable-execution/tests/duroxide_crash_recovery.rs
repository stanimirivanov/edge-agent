//! Real worker-process proofs with a disposable local PostgreSQL container.

use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use duroxide::{Client, OrchestrationStatus};
use duroxide_pg::PostgresProvider;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

#[path = "../../support/docker.rs"]
mod docker;

const POSTGRES_IMAGE: &str = "postgres:18.6-alpine3.23@sha256:885cf05d376c7cf27afef02073e6bdac3841252537f16e244fd1c1e6a7c99fb1";

async fn database() -> io::Result<(docker::Container, String)> {
    let postgres = docker::Container::start(
        POSTGRES_IMAGE,
        5432,
        &[
            ("POSTGRES_DB", "m08"),
            ("POSTGRES_USER", "m08"),
            ("POSTGRES_PASSWORD", "m08"),
        ],
        &[],
        // The image's temporary initialization server listens only on a Unix socket.
        "listening on IPv4 address \"0.0.0.0\", port 5432",
    )
    .await?;
    let url = format!("postgresql://m08:m08@127.0.0.1:{}/m08", postgres.port);
    Ok((postgres, url))
}

fn attempt_count(path: &std::path::Path) -> Result<usize, Box<dyn Error>> {
    Ok(fs::read_to_string(path)?.lines().count())
}

async fn start_worker_until_b(port: u16, evidence_dir: &Path) -> Result<Child, Box<dyn Error>> {
    let mut worker = Command::new(env!("CARGO_BIN_EXE_duroxide_worker"))
        .arg(port.to_string())
        .arg(evidence_dir)
        .arg("crash")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = worker.stdout.take().ok_or("worker stdout unavailable")?;
    let mut lines = BufReader::new(stdout).lines();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match lines.next_line().await? {
                Some(line) if line == "STEP_B_STARTED" => return Ok::<_, io::Error>(()),
                Some(_) => continue,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "worker exited before Step B",
                    ));
                }
            }
        }
    })
    .await??;
    Ok(worker)
}

#[tokio::test]
async fn completed_step_is_not_reexecuted_after_worker_process_crash() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let (postgres, database_url) = database().await?;
    let store = Arc::new(PostgresProvider::new(&database_url).await?);
    let client = Client::new(store);
    client
        .start_orchestration("m08-synthetic", "M08", "synthetic")
        .await?;
    let duplicate_start = client
        .start_orchestration("m08-synthetic", "M08", "conflicting-input")
        .await;
    assert!(
        duplicate_start.is_ok(),
        "Duroxide admits a second queued start before a worker creates the instance"
    );

    let worker = env!("CARGO_BIN_EXE_duroxide_worker");
    let mut first = start_worker_until_b(postgres.port, temp.path()).await?;

    // This is an abrupt process death during B, not a panic caught by Tokio
    // or a graceful worker shutdown. The history database survives it.
    first.kill().await?;
    let first_status = first.wait().await?;
    assert!(!first_status.success());

    let second_status = tokio::time::timeout(Duration::from_secs(30), async {
        Command::new(worker)
            .arg(postgres.port.to_string())
            .arg(temp.path())
            .arg("resume")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .status()
            .await
    })
    .await??;
    assert!(
        second_status.success(),
        "resumed worker failed: {second_status}"
    );

    let final_status = client.get_orchestration_status("m08-synthetic").await?;
    assert!(
        matches!(final_status, OrchestrationStatus::Completed { output, .. } if output == "model-result:synthetic"),
        "the restarted worker must complete the same durable instance"
    );
    assert_eq!(
        attempt_count(&temp.path().join("step_a_attempts"))?,
        1,
        "duplicate starts must not re-execute Step A or replace the original input"
    );
    assert_eq!(attempt_count(&temp.path().join("step_b_attempts"))?, 2);
    Ok(())
}

#[tokio::test]
async fn cancellation_of_in_flight_b_is_terminal_without_completing_b() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let (postgres, database_url) = database().await?;
    let store = Arc::new(PostgresProvider::new(&database_url).await?);
    let client = Client::new(store);
    client
        .start_orchestration("m08-synthetic", "M08", "synthetic")
        .await?;
    let mut worker = start_worker_until_b(postgres.port, temp.path()).await?;

    client
        .cancel_instance("m08-synthetic", "test cancellation")
        .await?;
    let status = client
        .wait_for_orchestration("m08-synthetic", Duration::from_secs(15))
        .await?;
    assert!(
        matches!(status, OrchestrationStatus::Failed { details, .. } if details.display_message().to_lowercase().contains("cancel")),
        "cancellation must be a terminal, observable result"
    );

    worker.kill().await?;
    worker.wait().await?;
    assert_eq!(attempt_count(&temp.path().join("step_a_attempts"))?, 1);
    assert_eq!(attempt_count(&temp.path().join("step_b_attempts"))?, 1);
    Ok(())
}
