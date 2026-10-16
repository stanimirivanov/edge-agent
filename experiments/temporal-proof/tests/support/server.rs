//! Own the verified CLI process without enabling the SDK's archive downloader.

use std::{error::Error, io, path::Path, process::Stdio, time::Duration};
use temporalio_client::{Client, ClientOptions, Connection, ConnectionOptions, Url};
use tokio::{process::Child, time::timeout};

pub(super) struct VerifiedServer {
    child: Child,
    client: Client,
}

impl VerifiedServer {
    pub(super) async fn start(executable: &Path, port: u16) -> Result<Self, Box<dyn Error>> {
        let mut child = tokio::process::Command::new(executable)
            .args([
                "server",
                "start-dev",
                "--ip",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--namespace",
                "default",
                "--headless",
                "--log-level",
                "error",
                "--dynamic-config-value",
                "frontend.enableServerVersionCheck=false",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let endpoint = Url::parse(&format!("http://127.0.0.1:{port}"))?;
        let options = ConnectionOptions::new(endpoint)
            .identity("m08-proof-readiness".to_owned())
            .connect_timeout(Duration::from_secs(1))
            .build();
        // Match the SDK fixture's five-second bound. Successful connection also
        // completes GetSystemInfo; a listening TCP socket alone is not readiness.
        let mut last_connection_error = None;
        let readiness = timeout(Duration::from_secs(5), async {
            loop {
                if let Some(status) = child.try_wait()? {
                    return Err(io::Error::other(format!(
                        "Temporal CLI exited before readiness: {status}"
                    ))
                    .into());
                }
                match Connection::connect(options.clone()).await {
                    Ok(connection) => {
                        return Client::new(connection, ClientOptions::new("default").build())
                            .map_err(Box::<dyn Error>::from);
                    }
                    Err(error) => {
                        last_connection_error = Some(error);
                        // Poll observed service readiness, never retry workflow work.
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        })
        .await;
        let client = match readiness {
            Ok(Ok(client)) => client,
            Ok(Err(error)) => {
                stop(&mut child).await?;
                return Err(error);
            }
            Err(error) => {
                stop(&mut child).await?;
                return Err(last_connection_error
                    .map_or_else(
                        || io::Error::new(io::ErrorKind::TimedOut, error),
                        |error| io::Error::new(io::ErrorKind::TimedOut, error),
                    )
                    .into());
            }
        };
        Ok(Self { child, client })
    }

    pub(super) fn client(&self) -> &Client {
        &self.client
    }

    pub(super) async fn shutdown(&mut self) -> Result<(), Box<dyn Error>> {
        stop(&mut self.child).await
    }
}

async fn stop(child: &mut Child) -> Result<(), Box<dyn Error>> {
    if child.try_wait()?.is_none()
        && let Err(error) = child.kill().await
        && child.try_wait()?.is_none()
    {
        // A child may exit between try_wait and kill. Only a still-live child
        // makes failed termination a cleanup failure.
        return Err(error.into());
    }
    let _ = child.wait().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_reaps_unwaited_and_completed_children() -> Result<(), Box<dyn Error>> {
        for complete_first in [false, true] {
            // Libtest's list command is a local, nonrecursive, service-free child.
            let mut child = tokio::process::Command::new(std::env::current_exe()?)
                .arg("--list")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            if complete_first {
                let _ = child.wait().await?;
            }
            stop(&mut child).await?;
            assert!(child.try_wait()?.is_some());
            assert!(child.id().is_none());
            stop(&mut child).await?;
        }
        Ok(())
    }
}
