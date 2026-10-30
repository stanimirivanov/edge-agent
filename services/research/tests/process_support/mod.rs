#![allow(
    clippy::disallowed_methods,
    reason = "Unique owned acceptance schemas and observed database fault boundaries."
)]
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    evidence::{self, AcquireEvidence, DailyEvidenceArtifact},
    research::ResearchRequest,
    research_acquisition,
};
use edgeagent_research_application::{
    lifecycle::CHANGED,
    publication::{ABSTAINED, PUBLISHED},
};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub struct Schema {
    pub name: String,
    pub pool: sqlx::PgPool,
}
pub struct Journey {
    pub gateway: Schema,
    pub research: Schema,
    pub projector: Schema,
    pub root: PathBuf,
    pub md_schema: String,
    pub stream: String,
    pub pg: String,
    pub nats: String,
    pub context: async_nats::jetstream::Context,
    pub gateway_address: std::net::SocketAddr,
    pub projector_address: std::net::SocketAddr,
    servers: Vec<tokio::process::Child>,
    root_pool: sqlx::PgPool,
    client: async_nats::Client,
}
fn binary(key: &str) -> Result<PathBuf> {
    let path = PathBuf::from(std::env::var(key)?);
    if !path.is_absolute() || !path.is_file() {
        return Err("process gate requires freshly built absolute binaries".into());
    }
    Ok(path)
}
async fn schema(
    root: &sqlx::PgPool,
    pg: &str,
    name: String,
    migrations: Vec<&'static str>,
) -> Result<Schema> {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {name}")))
        .execute(root)
        .await?;
    let options: sqlx::postgres::PgConnectOptions = pg.parse()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options.options([("search_path", name.as_str())]))
        .await?;
    for sql in migrations {
        sqlx::raw_sql(sql).execute(&pool).await?;
    }
    Ok(Schema { name, pool })
}
fn address() -> Result<std::net::SocketAddr> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?)
}
impl Journey {
    pub async fn new() -> Result<Self> {
        let pg = std::env::var("EDGEAGENT_POSTGRES_URL")?;
        let nats = std::env::var("EDGEAGENT_NATS_URL")?;
        edgeagent_research::runtime::local(&pg, &["postgres", "postgresql"])?;
        edgeagent_research::runtime::local(&nats, &["nats", "tls"])?;
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let suffix = format!(
            "{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        );
        let root = std::env::temp_dir().join(format!("research-processes-{suffix}"));
        let md_schema = format!(
            "edgeagent_offline_{}",
            &DailyEvidenceArtifact::hash(root.to_str().ok_or("path")?.as_bytes()).as_str()[..32]
        );
        let root_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect(&pg)
            .await?;
        let client = async_nats::connect(&nats).await?;
        let context = async_nats::jetstream::new(client.clone());
        let stream = format!("research_processes_{suffix}");
        std::fs::create_dir(&root)?;
        let mut stream_created = false;
        let setup: Result<Self> = async {
            let outbox = edgeagent_outbox_postgres::PostgresOutbox::MIGRATIONS;
            let inbox = edgeagent_inbox_postgres::PostgresInbox::MIGRATIONS;
            let gateway = schema(
                &root_pool,
                &pg,
                format!("gateway_{suffix}"),
                outbox
                    .into_iter()
                    .chain([include_str!(
                        "../../../gateway/migrations/0001_gateway_requests.sql"
                    )])
                    .collect(),
            )
            .await?;
            let research = schema(
                &root_pool,
                &pg,
                format!("research_{suffix}"),
                inbox
                    .into_iter()
                    .chain(outbox)
                    .chain([
                        edgeagent_research_postgres::MIGRATION,
                        edgeagent_research_postgres::lifecycle::MIGRATION,
                    ])
                    .collect(),
            )
            .await?;
            let projector = schema(
                &root_pool,
                &pg,
                format!("projector_{suffix}"),
                inbox
                    .into_iter()
                    .chain([include_str!(
                        "../../../audit-projector/migrations/0001_research_projection.sql"
                    )])
                    .collect(),
            )
            .await?;
            let mut subjects = vec![
                research_acquisition::REQUEST.subject()?,
                research_acquisition::DECIDED.subject()?,
                PUBLISHED.subject()?,
                ABSTAINED.subject()?,
                CHANGED.subject()?,
            ];
            subjects.extend(
                evidence::DEFINITIONS
                    .iter()
                    .map(|d| d.subject())
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            );
            subjects.sort();
            subjects.dedup();
            let actual = context
                .create_stream(async_nats::jetstream::stream::Config {
                    name: stream.clone(),
                    subjects,
                    max_messages: 128,
                    max_bytes: 33_554_432,
                    max_message_size: 262_144,
                    max_age: Duration::from_secs(600),
                    storage: async_nats::jetstream::stream::StorageType::File,
                    ..Default::default()
                })
                .await?;
            stream_created = true;
            for (name, filters) in [
                ("acquisition", vec![evidence::DEFINITIONS[0].subject()?]),
                ("market", vec![research_acquisition::REQUEST.subject()?]),
                ("research", vec![research_acquisition::DECIDED.subject()?]),
                (
                    "projector",
                    vec![
                        PUBLISHED.subject()?,
                        ABSTAINED.subject()?,
                        CHANGED.subject()?,
                    ],
                ),
            ] {
                let mut config = async_nats::jetstream::consumer::pull::Config {
                    durable_name: Some(name.into()),
                    ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                    ack_wait: Duration::from_secs(60),
                    max_deliver: 3,
                    max_ack_pending: 8,
                    ..Default::default()
                };
                if filters.len() == 1 {
                    config.filter_subject = filters[0].clone();
                } else {
                    config.filter_subjects = filters;
                }
                actual.create_consumer(config).await?;
            }
            Ok(Self {
                gateway,
                research,
                projector,
                root: root.clone(),
                md_schema: md_schema.clone(),
                stream: stream.clone(),
                pg: pg.clone(),
                nats: nats.clone(),
                context: context.clone(),
                client: client.clone(),
                root_pool: root_pool.clone(),
                gateway_address: address()?,
                projector_address: address()?,
                servers: Vec::new(),
            })
        }
        .await;
        if setup.is_err() {
            if stream_created {
                context.delete_stream(&stream).await?;
            }
            for name in [
                format!("gateway_{suffix}"),
                format!("research_{suffix}"),
                format!("projector_{suffix}"),
            ] {
                sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "DROP SCHEMA IF EXISTS {name} CASCADE"
                )))
                .execute(&root_pool)
                .await?;
            }
            root_pool.close().await;
            client.drain().await?;
            std::fs::remove_dir_all(&root)?;
        }
        setup
    }
    pub fn command(&self, service: &str, mode: &str) -> Result<tokio::process::Command> {
        let key = match service {
            "gateway" => "EDGEAGENT_PROCESS_GATEWAY",
            "market" => "EDGEAGENT_PROCESS_MARKET_DATA",
            "research" => "EDGEAGENT_PROCESS_RESEARCH",
            _ => "EDGEAGENT_PROCESS_PROJECTOR",
        };
        let mut command = tokio::process::Command::new(binary(key)?);
        command
            .arg(mode)
            .env("EDGEAGENT_POSTGRES_URL", &self.pg)
            .env("EDGEAGENT_NATS_URL", &self.nats)
            .env("EDGEAGENT_GATEWAY_SCHEMA", &self.gateway.name)
            .env("EDGEAGENT_GATEWAY_SCOPE", "operator")
            .env("EDGEAGENT_GATEWAY_LISTEN", self.gateway_address.to_string())
            .env(
                "EDGEAGENT_GATEWAY_PROJECTOR_URL",
                format!("http://{}", self.projector_address),
            )
            .env("USE_MOCK", "1")
            .env("EDGEAGENT_MARKET_DATA_PROVIDER", "synthetic-daily-v1")
            .env("EDGEAGENT_OFFLINE_ROOT", &self.root)
            .env("EDGEAGENT_OFFLINE_RESEARCH_SCOPE", "operator")
            .env("EDGEAGENT_OFFLINE_STREAM", &self.stream)
            .env(
                "EDGEAGENT_OFFLINE_CONSUMER",
                if mode == "offline-worker" {
                    "acquisition"
                } else {
                    "market"
                },
            )
            .env(
                "EDGEAGENT_OFFLINE_MAX_MESSAGES",
                if mode == "offline-relay" { "16" } else { "1" },
            )
            .env("EDGEAGENT_RESEARCH_SCHEMA", &self.research.name)
            .env("EDGEAGENT_RESEARCH_SCOPE", "operator")
            .env("EDGEAGENT_RESEARCH_STREAM", &self.stream)
            .env("EDGEAGENT_RESEARCH_CONSUMER", "research")
            .env(
                "EDGEAGENT_RESEARCH_MAX_MESSAGES",
                if mode == "offline-relay" { "16" } else { "1" },
            )
            .env(
                "EDGEAGENT_RESEARCH_OWNER_BINARY",
                binary("EDGEAGENT_PROCESS_MARKET_DATA")?,
            )
            .env("EDGEAGENT_RESEARCH_OWNER_ROOT", &self.root)
            .env("EDGEAGENT_RESEARCH_OWNER_DATABASE_URL", &self.pg)
            .env(
                "EDGEAGENT_RESEARCH_OWNER_ACQUISITION",
                format!("{:032x}", 1),
            )
            .env("EDGEAGENT_RESEARCH_IMPLEMENTATION", "02".repeat(32))
            .env("EDGEAGENT_RESEARCH_CONFIGURATION", "03".repeat(32))
            .env("EDGEAGENT_RESEARCH_MAX_AGE_MS", "1209600000")
            .env("EDGEAGENT_RESEARCH_TTL_MS", "60000")
            .env("EDGEAGENT_PROJECTOR_SCHEMA", &self.projector.name)
            .env("EDGEAGENT_PROJECTOR_SCOPE", "operator")
            .env("EDGEAGENT_PROJECTOR_STREAM", &self.stream)
            .env("EDGEAGENT_PROJECTOR_CONSUMER", "projector")
            .env("EDGEAGENT_PROJECTOR_MAX_MESSAGES", "1")
            .env(
                "EDGEAGENT_PROJECTOR_LISTEN",
                self.projector_address.to_string(),
            )
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(command)
    }
    pub async fn run(&self, service: &str, mode: &str, args: &[&str]) -> Result<Vec<u8>> {
        let output = tokio::time::timeout(
            Duration::from_secs(60),
            self.command(service, mode)?.args(args).output(),
        )
        .await??;
        if !output.status.success() {
            return Err(format!(
                "{service} {mode} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(output.stdout)
    }
    pub async fn start_servers(&mut self) -> Result {
        for (service, address) in [
            ("projector", self.projector_address),
            ("gateway", self.gateway_address),
        ] {
            let child = self
                .command(service, "serve")?
                .stdout(Stdio::null())
                .spawn()?;
            self.servers.push(child);
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if tokio::net::TcpStream::connect(address).await.is_ok() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await?;
        }
        Ok(())
    }
    pub async fn request(&self, method: &str, path: &str, body: &[u8]) -> Result<(u16, Vec<u8>)> {
        tokio::time::timeout(Duration::from_secs(25),async{
            let mut socket=tokio::net::TcpStream::connect(self.gateway_address).await?;
            socket.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",self.gateway_address,body.len()).as_bytes()).await?;socket.write_all(body).await?;
            let mut result=Vec::new();socket.take(262_145).read_to_end(&mut result).await?;
            let split=result.windows(4).position(|w|w==b"\r\n\r\n").ok_or("HTTP framing")?;
            let status=std::str::from_utf8(&result[..split])?.split_whitespace().nth(1).ok_or("HTTP status")?.parse()?;Ok((status,result[split+4..].to_vec()))
        }).await?
    }
    pub async fn input(&self, service: &str, mode: &str, input: &[u8]) -> Result<Vec<u8>> {
        let mut child = self
            .command(service, mode)?
            .env(
                "EDGEAGENT_RESEARCH_REPLAY_RENDERER",
                edgeagent_research_application::RENDERER_REVISION,
            )
            .env(
                "EDGEAGENT_RESEARCH_REPLAY_MOMENTUM",
                edgeagent_research_domain::momentum::REVISION,
            )
            .stdin(Stdio::piped())
            .spawn()?;
        child.stdin.take().ok_or("stdin")?.write_all(input).await?;
        let output =
            tokio::time::timeout(Duration::from_secs(40), child.wait_with_output()).await??;
        if !output.status.success() {
            return Err(format!(
                "{service} {mode}: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(output.stdout)
    }
    pub async fn rebuild(&mut self) -> Result {
        let mut child = self.servers.remove(0);
        child.start_kill()?;
        tokio::time::timeout(Duration::from_secs(5), child.wait()).await??;
        sqlx::raw_sql("TRUNCATE edgeagent_research_views,edgeagent_message_inbox")
            .execute(&self.projector.pool)
            .await?;
        let stream = self.context.get_stream(&self.stream).await?;
        stream.delete_consumer("projector").await?;
        stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("projector".into()),
                filter_subjects: vec![
                    PUBLISHED.subject()?,
                    ABSTAINED.subject()?,
                    CHANGED.subject()?,
                ],
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(60),
                max_deliver: 3,
                max_ack_pending: 8,
                ..Default::default()
            })
            .await?;
        self.servers.push(
            self.command("projector", "serve")?
                .stdout(Stdio::null())
                .spawn()?,
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if tokio::net::TcpStream::connect(self.projector_address)
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await?;
        Ok(())
    }
    pub async fn gateway_relay(&self) -> Result {
        use edgeagent_messaging::LeaseDuration;
        use edgeagent_outbox_relay::{MaxAttempts, RetryBackoff};
        let mut connection = self.gateway.pool.acquire().await?;
        let mut store = edgeagent_outbox_postgres::PostgresOutboxRelay::new(&mut connection);
        let publisher =
            edgeagent_messaging_nats::JetStreamPublisher::from_context(self.context.clone());
        let policy = edgeagent_outbox_relay::RelayPolicy::new(
            "qualification-gateway",
            LeaseDuration::new(Duration::from_secs(30))?,
            MaxAttempts::new(3)?,
            RetryBackoff::new(Duration::from_secs(1), Duration::from_secs(60))?,
        )?;
        let result = edgeagent_outbox_relay::relay_once(
            &mut store,
            &research_acquisition::registry()?,
            &publisher,
            &policy,
        )
        .await?;
        if !matches!(
            result,
            edgeagent_outbox_relay::RelayOutcome::Published { .. }
        ) {
            return Err(format!("Gateway relay did not publish: {result:?}").into());
        }
        Ok(())
    }
    pub async fn seed_history(&self) -> Result {
        self.run("market", "offline-init", &[]).await?;
        let bytes = self
            .run(
                "market",
                "offline-seed",
                &[&format!("{:032x}", 1), "history-v1"],
            )
            .await?;
        let payload: AcquireEvidence = serde_json::from_slice(&bytes)?;
        let def = evidence::DEFINITIONS[0];
        let partition = format!("acquisition/{}", payload.request.as_str());
        let envelope = MessageEnvelope::from_payload(
            MessageMetadata {
                id: "qualification-acquire".into(),
                source: Component::MarketData.source_uri().into(),
                message_type: def.message_type().into(),
                subject: partition.clone(),
                time: "2026-10-30T00:00:00Z".into(),
                data_schema: def.data_schema().into(),
                correlation_id: "qualification".into(),
                causation_id: "seed".into(),
                idempotency_key: payload.request_sha256.as_str().into(),
                partition_key: partition,
                trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
                trace_state: None,
            },
            &payload,
        )?;
        self.context
            .publish(def.subject()?, envelope.to_json()?.into())
            .await?
            .await?;
        self.run("market", "offline-worker", &[]).await?;
        Ok(())
    }
    pub fn intent(&self, lookback: u16) -> Result<ResearchRequest> {
        Ok(ResearchRequest {
            scope: "operator".into(),
            request_id: "qualification".into(),
            implementation: "02".repeat(32),
            configuration: "03".repeat(32),
            decision_at_ms: i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis(),
            )?,
            lookback_sessions: lookback,
            instruments: vec![format!("{:032x}", 1)],
        })
    }
    pub async fn consume_market(&self) -> Result {
        self.run(
            "market",
            "offline-research-worker",
            &[&format!("{:032x}", 1)],
        )
        .await?;
        self.run("market", "offline-relay", &[]).await?;
        Ok(())
    }
    pub async fn finish(
        mut self,
        result: std::result::Result<Result, Box<dyn std::any::Any + Send>>,
    ) -> Result {
        for child in &mut self.servers {
            child.start_kill()?;
            tokio::time::timeout(Duration::from_secs(5), child.wait()).await??;
        }
        self.context.delete_stream(&self.stream).await?;
        self.client.drain().await?;
        self.gateway.pool.close().await;
        self.research.pool.close().await;
        self.projector.pool.close().await;
        for name in [
            &self.gateway.name,
            &self.research.name,
            &self.projector.name,
            &self.md_schema,
        ] {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP SCHEMA IF EXISTS {name} CASCADE"
            )))
            .execute(&self.root_pool)
            .await?;
        }
        self.root_pool.close().await;
        std::fs::remove_dir_all(&self.root)?;
        match result {
            Ok(r) => r,
            Err(p) => std::panic::resume_unwind(p),
        }
    }
}
