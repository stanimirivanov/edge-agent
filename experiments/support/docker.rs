//! Disposable local-Docker dependencies for the isolated M08 proofs.
//! No archive handling or Docker API client dependency is needed by these tests.

use std::{
    io,
    process::Command as CleanupCommand,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{process::Command, time::timeout};

static NEXT_CONTAINER: AtomicU64 = AtomicU64::new(0);

pub struct Container {
    name: String,
    pub port: u16,
}

impl Container {
    pub async fn start(
        image: &str,
        port: u16,
        environment: &[(&str, &str)],
        arguments: &[&str],
        ready_message: &str,
    ) -> io::Result<Self> {
        // Allocate the cleanup guard before creation, so failed startup and
        // readiness checks also remove only this invocation's named container.
        let mut container = Self {
            name: format!(
                "edgeagent-m08-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_CONTAINER.fetch_add(1, Ordering::Relaxed)
            ),
            port: 0,
        };
        let mapping = format!("127.0.0.1::{port}");
        let mut args = vec![
            "run",
            "--detach",
            "--name",
            &container.name,
            "--publish",
            &mapping,
        ];
        let variables: Vec<_> = environment
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        for variable in &variables {
            args.extend(["--env", variable]);
        }
        args.push(image);
        args.extend_from_slice(arguments);
        docker(&args).await?;
        let exposed = format!("{port}/tcp");
        let mapping = docker(&["port", &container.name, &exposed]).await?;
        container.port = mapping
            .trim()
            .strip_prefix("127.0.0.1:")
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| io::Error::other("Docker returned an invalid loopback port"))?;
        timeout(Duration::from_secs(60), async {
            loop {
                let logs = docker(&["logs", &container.name]).await?;
                // PostgreSQL's temporary initialization server has no TCP
                // listener. Require both the log marker and the published port.
                if logs.contains(ready_message)
                    && tokio::net::TcpStream::connect(("127.0.0.1", container.port))
                        .await
                        .is_ok()
                {
                    return Ok::<_, io::Error>(());
                }
                let running =
                    docker(&["inspect", "--format", "{{.State.Running}}", &container.name]).await?;
                if running.trim() != "true" {
                    return Err(io::Error::other(format!("proof container exited: {logs}")));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "proof container readiness timed out",
            )
        })??;
        Ok(container)
    }
}

async fn docker(arguments: &[&str]) -> io::Result<String> {
    let output = timeout(
        Duration::from_secs(180),
        Command::new("docker")
            .args(arguments)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Docker command timed out"))??;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Docker {} failed: {}",
            arguments[0],
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    // Docker logs writes service stderr to CLI stderr as well.
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

impl Drop for Container {
    fn drop(&mut self) {
        // Synchronous cleanup also works when the test's async future unwinds.
        let _ = CleanupCommand::new("docker")
            .args(["rm", "--force", "--volumes", &self.name])
            .output();
    }
}
