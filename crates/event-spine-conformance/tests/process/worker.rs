//! Owned finite subprocess protocol; stdin barriers never hold an adapter tx.

use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

use super::TestError;

pub struct Worker {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    deadline: tokio::time::Instant,
}

impl Worker {
    pub async fn spawn(
        role: &str,
        database_url: &str,
        schema: &str,
        component: &str,
        url: &str,
    ) -> Result<Self, TestError> {
        Self::spawn_observed(role, database_url, schema, component, url, None).await
    }

    pub async fn spawn_observed(
        role: &str,
        database_url: &str,
        schema: &str,
        component: &str,
        url: &str,
        telemetry: Option<&str>,
    ) -> Result<Self, TestError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut command = Command::new(env!("CARGO_BIN_EXE_conformance-worker"));
        command.env_clear();
        if let Some(directory) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", directory);
        }
        if let Some(endpoint) = telemetry {
            command.env("EDGEAGENT_PROOF_OTLP_ENDPOINT", endpoint);
        }
        let mut child = command
            .arg(role)
            .env("EDGEAGENT_POSTGRES_URL", database_url)
            .env("EDGEAGENT_PROOF_SCHEMA", schema)
            .env("EDGEAGENT_PROOF_NAMESPACE", "m02-process")
            .env("EDGEAGENT_PROOF_COMPONENT", component)
            .env("EDGEAGENT_NATS_URL", url)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().ok_or("child stdout missing")?);
        let mut worker = Self {
            child,
            input,
            output,
            deadline,
        };
        worker.expect("ready").await?;
        Ok(worker)
    }

    pub fn application_name(&self) -> Result<String, TestError> {
        Ok(format!(
            "m02_worker_{}",
            self.child.id().ok_or("child exited")?
        ))
    }

    pub async fn begin(&mut self, action: u8, bytes: &[u8]) -> Result<(), TestError> {
        let deadline = self
            .deadline
            .min(tokio::time::Instant::now() + Duration::from_secs(5));
        let result = tokio::time::timeout_at(deadline, self.write(action, bytes)).await;
        if !matches!(result, Ok(Ok(()))) {
            self.stop().await?;
        }
        result?
    }

    async fn write(&mut self, action: u8, bytes: &[u8]) -> Result<(), TestError> {
        let input = self.input.as_mut().ok_or("child stdin missing")?;
        input.write_all(&[action]).await?;
        input.write_all(bytes).await?;
        input.flush().await?;
        if !matches!(action, b'h' | b'v' | b'p' | b'b' | b'e' | b'u' | b'a') {
            self.input.take(); // Producer framing ends at EOF.
        }
        Ok(())
    }

    pub async fn expect(&mut self, expected: &str) -> Result<(), TestError> {
        match self.line().await {
            Ok(line) if line == expected => Ok(()),
            _ => {
                self.stop().await?;
                Err("worker protocol barrier failed".into())
            }
        }
    }

    pub async fn line(&mut self) -> Result<String, TestError> {
        let mut line = String::new();
        tokio::time::timeout_at(self.deadline, self.output.read_line(&mut line)).await??;
        Ok(line.trim().to_owned())
    }

    pub async fn finish(mut self) -> Result<String, TestError> {
        let result = tokio::time::timeout_at(self.deadline, async {
            let outcome = self.line().await?;
            if !self.child.wait().await?.success() {
                return Err::<String, TestError>("worker returned unresolved failure".into());
            }
            Ok(outcome)
        })
        .await;
        if !matches!(result, Ok(Ok(_))) {
            self.stop().await?;
        }
        result?
    }

    async fn stop(&mut self) -> Result<(), TestError> {
        if self.child.try_wait()?.is_none() {
            tokio::time::timeout(Duration::from_secs(5), self.child.kill()).await??;
        }
        Ok(())
    }

    pub async fn kill(mut self) -> Result<(), TestError> {
        self.stop().await
    }
}
