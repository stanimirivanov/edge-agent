use edgeagent_temporal_proof::{M08Input, M08Workflow};
use std::{
    error::Error,
    ffi::OsStr,
    fs::File,
    io::{self, Cursor},
    net::TcpListener,
    path::{Path, PathBuf},
    time::Duration,
};
use temporalio_client::{
    WorkflowFetchHistoryOptions, WorkflowGetResultOptions, WorkflowStartOptions,
};
use temporalio_sdk::{
    testing::{EphemeralExe, LocalWorkflowEnvironmentOptions, WorkflowEnvironment},
    workflow_replayer::{WorkflowReplayer, WorkflowReplayerOptions},
};
use tokio::{process::Command, time::timeout};

const QUEUE: &str = "synthetic-m08-evaluation";
const CLI_VERSION: &str = "1.8.3";
// SHA-256 of the official v1.8.3 release tarballs, published in checksums.txt.
// https://github.com/temporalio/cli/releases/download/v1.8.3/checksums.txt
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const CLI_ARCHIVE_SHA256: &str = "9aab450b009528052fda8645314045d2c84ef50f744df131d80ee539e28a8235";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const CLI_ARCHIVE_SHA256: &str = "6f0afac1e9ddea71f480c43a49f5db5167a244c21db923707f069a79bcabdfea";

async fn verified_temporal_cli(directory: &Path) -> Result<PathBuf, Box<dyn Error>> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    let (platform, executable) = ("windows", "temporal.exe");
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let (platform, executable) = ("linux", "temporal");
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64")
    )))]
    compile_error!("Temporal M08 proof requires a pinned CLI hash for this platform");

    let archive_name = format!("temporal_cli_{CLI_VERSION}_{platform}_amd64.tar.gz");
    let url = format!(
        "https://github.com/temporalio/cli/releases/download/v{CLI_VERSION}/{archive_name}"
    );
    let archive = reqwest::Client::new()
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &archive);
    let actual_sha256 = digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual_sha256 != CLI_ARCHIVE_SHA256 {
        return Err(io::Error::other(format!(
            "Temporal CLI archive SHA-256 mismatch for {archive_name}: {actual_sha256}"
        ))
        .into());
    }

    let path = directory.join(executable);
    let decoder = flate2::read::GzDecoder::new(Cursor::new(archive));
    let mut tarball = tar::Archive::new(decoder);
    let mut extracted = false;
    for entry in tarball.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type().is_file()
            && entry.path()?.file_name() == Some(OsStr::new(executable))
        {
            let mut output = File::create(&path)?;
            io::copy(&mut entry, &mut output)?;
            output.sync_all()?;
            extracted = true;
            break;
        }
    }
    if !extracted {
        return Err(io::Error::other(format!(
            "verified Temporal CLI archive did not contain {executable}"
        ))
        .into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions)?;
    }
    Ok(path)
}

fn unused_port() -> Result<u16, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn spawn_worker(endpoint: &str) -> Result<tokio::process::Child, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_temporal-proof-worker"))
        .arg(endpoint)
        .arg(QUEUE)
        .kill_on_drop(true)
        .spawn()?)
}

fn count_lines(path: &Path) -> Result<usize, Box<dyn Error>> {
    Ok(std::fs::read_to_string(path)?.lines().count())
}

#[tokio::test]
async fn worker_crash_during_model_step_resumes_without_reexecuting_preparation()
-> Result<(), Box<dyn Error>> {
    let evidence = tempfile::tempdir()?;
    let cli_directory = tempfile::tempdir()?;
    let cli = verified_temporal_cli(cli_directory.path()).await?;
    let port = unused_port()?;
    // The SDK owns the server lifecycle but never downloads executable bytes.
    let environment = WorkflowEnvironment::start_local(
        LocalWorkflowEnvironmentOptions::builder()
            .server_executable(EphemeralExe::ExistingPath(
                cli.to_str().ok_or("non-UTF-8 CLI path")?.to_owned(),
            ))
            .port(port)
            .build(),
    )
    .await?;
    let endpoint = format!("http://127.0.0.1:{port}");
    let input = M08Input {
        manifest_id: "synthetic-snapshot-r1".to_owned(),
        evidence_dir: evidence
            .path()
            .to_str()
            .ok_or("non-UTF-8 test path")?
            .to_owned(),
    };
    let mut first_worker = spawn_worker(&endpoint)?;
    let handle = environment
        .client()
        .start_workflow(
            M08Workflow::run,
            input,
            WorkflowStartOptions::new(QUEUE, "m08-synthetic-immutable-snapshot").build(),
        )
        .await?;

    let first_exit = timeout(Duration::from_secs(90), first_worker.wait()).await??;
    assert!(
        !first_exit.success(),
        "Step B must crash the first worker process"
    );
    assert!(evidence.path().join("step-b-first-attempt").exists());
    assert_eq!(count_lines(&evidence.path().join("step-a-invocations"))?, 1);

    let mut restarted_worker = spawn_worker(&endpoint)?;
    let result = timeout(
        Duration::from_secs(90),
        handle.get_result(WorkflowGetResultOptions::default()),
    )
    .await??;
    assert_eq!(result, "synthetic-snapshot-r1:prepared:evaluated");
    assert!(evidence.path().join("step-b-completed").exists());
    assert_eq!(
        count_lines(&evidence.path().join("step-a-invocations"))?,
        1,
        "a completed activity must not be re-executed after worker restart"
    );

    // Replay the actual server history against the same workflow definition.
    let replayer = WorkflowReplayer::new(
        WorkflowReplayerOptions::new()
            .register_workflow::<M08Workflow>()?
            .build(),
    )?;
    replayer
        .replay_workflow(handle.fetch_history(WorkflowFetchHistoryOptions::default()))
        .await?;
    restarted_worker.kill().await?;
    let _ = restarted_worker.wait().await?;
    environment.shutdown().await?;
    Ok(())
}
