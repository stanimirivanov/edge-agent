use super::*;
use edgeagent_contracts::{Component, MessageEnvelope, MessageMetadata, evidence::*};
use edgeagent_market_data_postgres::PostgresAcquisitionLedger;
use edgeagent_messaging_nats::topology::{Binding, TopologyLimits, TopologyPlan};
use futures_util::{FutureExt, TryStreamExt};
use object_store::ObjectStore;
use std::{
    future::Future,
    panic::AssertUnwindSafe,
    path::PathBuf,
    pin::Pin,
    process::{ExitStatus, Stdio},
    sync::Arc,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStderr, ChildStdout, Command},
    sync::Mutex,
};

pub fn postgres_url(value: &str) -> Result<url::Url, TestError> {
    let url = url::Url::parse(value)?;
    if !matches!(url.scheme(), "postgres" | "postgresql")
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("recovery fixtures require a plain loopback PostgreSQL URL".into());
    }
    Ok(url)
}

struct Process {
    child: Child,
    stdout: BufReader<ChildStdout>,
    stderr: ChildStderr,
}
pub struct Fixture {
    pub pg: String,
    pub schema: String,
    pub pool: sqlx::PgPool,
    root: PathBuf,
    broker: Arc<Mutex<broker::Broker>>,
    context: async_nats::jetstream::Context,
    stream: String,
    worker_url: String,
    processes: Vec<Process>,
}
impl Fixture {
    async fn new(broker: Arc<Mutex<broker::Broker>>) -> Result<Self, TestError> {
        let pg = std::env::var("EDGEAGENT_POSTGRES_URL")?;
        postgres_url(&pg)?; // Reject routing overrides/remote accounts BEFORE any fixture connection.
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "edgeagent-recovery-{}-{unique}",
            std::process::id()
        ));
        let options: sqlx::postgres::PgConnectOptions = pg.parse()?;
        // Connect before creating local resources; a missing prerequisite owns no directory.
        let admin = sqlx::PgPool::connect_with(options.clone()).await?;
        std::fs::create_dir(&root)?;
        let initialized = async {
            let canonical = std::fs::canonicalize(&root)?;
            let schema = format!(
                "edgeagent_offline_{}",
                &DailyEvidenceArtifact::hash(canonical.to_str().ok_or("root encoding")?.as_bytes())
                    .as_str()[..32]
            );
            admin.close().await;
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(3)
                .acquire_timeout(Duration::from_secs(5))
                .connect_with(options.options([
                    ("search_path", schema.as_str()),
                    ("statement_timeout", "10000"),
                    ("lock_timeout", "5000"),
                ]))
                .await?;
            let url = broker.lock().await.url().to_owned();
            let context = async_nats::jetstream::new(
                broker::connect(&url, "inspector", "_INBOX.inspector").await?,
            );
            Ok(Self {
                pg,
                schema,
                pool,
                root: root.clone(),
                broker,
                context,
                worker_url: url,
                stream: format!("recovery_{unique}"),
                processes: Vec::new(),
            })
        }
        .await;
        if initialized.is_err() {
            // Constructor failure precedes migrations/profile writes: this owned directory is empty.
            std::fs::remove_dir(&root)?;
        }
        initialized
    }
    pub async fn broker_url(&self) -> String {
        self.broker.lock().await.url().to_owned()
    }
    pub async fn restart_broker(&mut self) -> TestResult {
        self.broker.lock().await.restart().await?;
        self.worker_url = self.broker_url().await;
        self.context = async_nats::jetstream::new(
            broker::connect(&self.broker_url().await, "inspector", "_INBOX.inspector").await?,
        );
        Ok(())
    }
    pub fn ledger(&self) -> PostgresAcquisitionLedger {
        PostgresAcquisitionLedger::new(self.pool.clone())
    }
    pub fn spawn(
        &mut self,
        args: &[&str],
        pg: Option<&str>,
        nats: Option<&str>,
    ) -> Result<usize, TestError> {
        // Only the connection endpoint is replaced by a loopback wire fixture.
        let mut command = Command::new(env!("CARGO_BIN_EXE_edgeagent-market-data"));
        command
            .args(args)
            .env("EDGEAGENT_POSTGRES_URL", pg.unwrap_or(&self.pg))
            .env("EDGEAGENT_NATS_URL", nats.unwrap_or(&self.worker_url))
            .env("EDGEAGENT_OFFLINE_ROOT", &self.root)
            .env("USE_MOCK", "1")
            .env("EDGEAGENT_MARKET_DATA_PROVIDER", "synthetic-daily-v1")
            .env("EDGEAGENT_OFFLINE_MAX_MESSAGES", "1")
            .env("EDGEAGENT_OFFLINE_STREAM", &self.stream)
            .env("EDGEAGENT_OFFLINE_CONSUMER", "offline");
        self.track(command)
    }
    fn track(&mut self, mut command: Command) -> Result<usize, TestError> {
        if self.processes.len() >= 12 {
            return Err("fixture process bound".into());
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = BufReader::new(child.stdout.take().ok_or("child stdout")?);
        let stderr = child.stderr.take().ok_or("child stderr")?;
        self.processes.push(Process {
            child,
            stdout,
            stderr,
        });
        Ok(self.processes.len() - 1)
    }
    pub fn quota_child(&mut self, command: &AcquireEvidence) -> Result<usize, TestError> {
        let mut child = Command::new(std::env::current_exe()?);
        child
            .args([
                "--exact",
                "quota_child_protocol_and_process_entry",
                "--nocapture",
            ])
            .env("EDGEAGENT_PR14_QUOTA_CHILD", "reserve")
            .env("EDGEAGENT_PR14_COMMAND", serde_json::to_string(command)?)
            .env("EDGEAGENT_POSTGRES_URL", &self.pg)
            .env("EDGEAGENT_PR14_SCHEMA", &self.schema);
        self.track(child)
    }
    pub async fn signal(&mut self, id: usize) -> TestResult {
        self.processes
            .get_mut(id)
            .ok_or("child identity")?
            .child
            .stdin
            .as_mut()
            .ok_or("child stdin")?
            .write_all(b"g")
            .await?;
        Ok(())
    }
    pub async fn marker(&mut self, id: usize) -> Result<String, TestError> {
        tokio::time::timeout(Duration::from_secs(30), async {
            let process = self.processes.get_mut(id).ok_or("child identity")?;
            loop {
                let mut line = Vec::new();
                if (&mut process.stdout)
                    .take(1025)
                    .read_until(b'\n', &mut line)
                    .await?
                    == 0
                {
                    return Err("quota child exited before barrier".into());
                }
                if line.len() > 1024 {
                    return Err("child output exceeded bound".into());
                }
                let line = std::str::from_utf8(&line)?;
                if let Some((_, marker)) = line.split_once("quota=") {
                    return Ok(marker.trim().to_owned());
                }
            }
        })
        .await?
    }
    pub async fn kill(&mut self, id: usize) -> TestResult {
        let process = self.processes.get_mut(id).ok_or("child identity")?;
        if process.child.try_wait()?.is_some() {
            return Err("child exited before forced-death barrier".into());
        }
        tokio::time::timeout(Duration::from_secs(10), process.child.kill()).await??;
        assert!(!process.child.wait().await?.success());
        Ok(())
    }
    pub async fn finish(&mut self, id: usize) -> Result<(ExitStatus, String), TestError> {
        tokio::time::timeout(Duration::from_secs(120), async {
            let process = self.processes.get_mut(id).ok_or("child identity")?;
            let mut output = Vec::new();
            (&mut process.stdout)
                .take(16385)
                .read_to_end(&mut output)
                .await?;
            if output.len() > 16384 {
                return Err("child output exceeded bound".into());
            }
            let mut errors = Vec::new();
            (&mut process.stderr)
                .take(4097)
                .read_to_end(&mut errors)
                .await?;
            if errors.len() > 4096 {
                return Err("child error output exceeded bound".into());
            }
            output.extend_from_slice(&errors);
            Ok((process.child.wait().await?, String::from_utf8(output)?))
        })
        .await?
    }
    pub async fn run(&mut self, args: &[&str]) -> Result<String, TestError> {
        let url = self.broker_url().await;
        let id = self.spawn(args, None, Some(&url))?;
        let (status, output) = self.finish(id).await?;
        if !status.success() {
            return Err(format!("owned offline process failed: {output}").into());
        }
        Ok(output)
    }
    async fn initialize(&mut self) -> TestResult {
        self.run(&["offline-init"]).await?;
        let stream = self
            .context
            .create_stream(async_nats::jetstream::stream::Config {
                name: self.stream.clone(),
                subjects: DEFINITIONS
                    .iter()
                    .map(|d| d.subject())
                    .collect::<Result<_, _>>()?,
                max_messages: 64,
                max_bytes: 16_777_216,
                max_message_size: 1_048_576,
                max_age: Duration::from_secs(600),
                storage: async_nats::jetstream::stream::StorageType::File,
                ..Default::default()
            })
            .await?;
        stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("offline".into()),
                filter_subject: DEFINITIONS[0].subject()?,
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(30),
                max_deliver: 3,
                max_ack_pending: 1,
                ..Default::default()
            })
            .await?;
        Ok(())
    }
    pub async fn seed(&mut self, id: u128) -> Result<AcquireEvidence, TestError> {
        Ok(serde_json::from_str(
            &self.run(&["offline-seed", &format!("{id:032x}")]).await?,
        )?)
    }
    pub async fn publish(
        &self,
        command: &AcquireEvidence,
        id: &str,
    ) -> Result<MessageEnvelope, TestError> {
        let d = DEFINITIONS[0];
        let delivery = MessageEnvelope::from_payload(
            MessageMetadata {
                id: id.into(),
                source: Component::Gateway.source_uri().into(),
                message_type: d.message_type().into(),
                subject: format!("acquisition/{}", command.request.as_str()),
                time: "2026-10-24T00:00:00Z".into(),
                data_schema: d.data_schema().into(),
                correlation_id: "offline-recovery".into(),
                causation_id: "fixture".into(),
                idempotency_key: command.request.as_str().into(),
                partition_key: format!("acquisition/{}", command.request.as_str()),
                trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
                trace_state: None,
            },
            command,
        )?;
        self.context
            .publish(d.subject()?, delivery.to_json()?.into())
            .await?
            .await?;
        Ok(delivery)
    }
    pub async fn durable(&self) -> Result<async_nats::jetstream::consumer::Info, TestError> {
        let stream = self.context.get_stream(&self.stream).await?;
        let mut consumer: async_nats::jetstream::consumer::PullConsumer =
            stream.get_consumer("offline").await?;
        Ok(consumer.info().await?.clone())
    }
    pub async fn counts(&self) -> Result<(i64, i64, i64), TestError> {
        Ok((
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_acquisition_receipts")
                .fetch_one(&self.pool)
                .await?,
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_inbox")
                .fetch_one(&self.pool)
                .await?,
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_outbox")
                .fetch_one(&self.pool)
                .await?,
        ))
    }
    pub async fn objects(&self) -> Result<usize, TestError> {
        let store =
            object_store::local::LocalFileSystem::new_with_prefix(self.root.join("objects"))?;
        Ok(store.list(None).try_collect::<Vec<_>>().await?.len())
    }
    pub async fn inventory(&self) -> Result<i64, TestError> {
        Ok(
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_evidence_objects")
                .fetch_one(&self.pool)
                .await?,
        )
    }
    async fn close(&mut self) -> TestResult {
        let mut failure = None;
        for process in &mut self.processes {
            let result: TestResult = async {
                if process.child.try_wait()?.is_none() {
                    tokio::time::timeout(Duration::from_secs(10), process.child.kill()).await??;
                }
                process.child.wait().await?;
                Ok(())
            }
            .await;
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        }
        if let Err(error) = sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            self.schema
        )))
        .execute(&self.pool)
        .await
        {
            failure.get_or_insert(error.into());
        }
        if let Err(error) = tokio::time::timeout(Duration::from_secs(15), self.pool.close()).await {
            failure.get_or_insert(error.into());
        }
        // Exclusive create_dir above established exact ownership, never a caller path.
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            failure.get_or_insert(error.into());
        }
        failure.map_or(Ok(()), Err)
    }
}
pub async fn with_case<F>(body: F) -> TestResult
where
    F: for<'a> FnOnce(&'a mut Fixture) -> Pin<Box<dyn Future<Output = TestResult> + Send + 'a>>,
{
    let plan = TopologyPlan::new(
        registry()?,
        "m03-recovery",
        &[Binding::new(
            Component::Gateway,
            DEFINITIONS[0].message_type(),
        )],
        &[Binding::new(
            Component::AuditProjector,
            DEFINITIONS[1].message_type(),
        )],
        TopologyLimits::default(),
    )?;
    // Reserve teardown time beyond the 220s body and twelve bounded child joins.
    broker::with_offline_broker(&plan, Duration::from_secs(400), |broker| async move {
        let mut fixture = Fixture::new(broker).await?;
        let result = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(220), async {
            fixture.initialize().await?;
            body(&mut fixture).await
        }))
        .catch_unwind()
        .await;
        let cleanup = fixture.close().await;
        match result {
            Ok(Ok(Ok(()))) => cleanup,
            Ok(Ok(Err(error))) => {
                cleanup?;
                Err(error)
            }
            Ok(Err(error)) => {
                cleanup?;
                Err(error.into())
            }
            Err(panic) => {
                cleanup?;
                std::panic::resume_unwind(panic)
            }
        }
    })
    .await
}
