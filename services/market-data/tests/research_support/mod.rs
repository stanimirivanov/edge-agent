#![allow(
    clippy::disallowed_methods,
    reason = "Owned isolated test fixture SQL."
)]
#![allow(
    dead_code,
    reason = "Shared by stdin and broker process proof targets."
)]
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    evidence::*,
    research::{ResearchRecord, ResearchRequest},
    research_acquisition,
};
use edgeagent_market_data_postgres::PostgresResearchEvidence;
use std::{path::Path, time::Duration};
use tokio::io::AsyncWriteExt;
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub struct Harness {
    pub root: std::path::PathBuf,
    pub pool: sqlx::PgPool,
    pub schema: String,
    pub pg: String,
}
impl Harness {
    pub async fn new() -> Result<Self> {
        let pg = std::env::var("EDGEAGENT_POSTGRES_URL")?;
        let url = url::Url::parse(&pg)?;
        if !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
            || url.query().is_some()
        {
            return Err("test database must be loopback without overrides".into());
        }
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "research-evidence-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let schema = format!(
            "edgeagent_offline_{}",
            &DailyEvidenceArtifact::hash(root.to_str().ok_or("path")?.as_bytes()).as_str()[..32]
        );
        let options: sqlx::postgres::PgConnectOptions = pg.parse()?;
        let pool =
            sqlx::PgPool::connect_with(options.options([("search_path", schema.as_str())])).await?;
        let h = Self {
            root,
            pool,
            schema,
            pg,
        };
        h.success(&["offline-init"], None).await?;
        Ok(h)
    }
    pub async fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<std::process::Output> {
        run(&self.root, &self.pg, args, input).await
    }
    pub async fn success(&self, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
        let output = self.run(args, input).await?;
        if !output.status.success() {
            return Err(
                format!("worker failed: {}", String::from_utf8_lossy(&output.stderr)).into(),
            );
        }
        Ok(output.stdout)
    }
    pub async fn seed(&self, recipe: &str) -> Result<ResearchRequest> {
        self.success(
            &["offline-seed", "00000000000000000000000000000001", recipe],
            None,
        )
        .await?;
        let record = PostgresResearchEvidence::new(self.pool.clone())
            .owner_record(&EvidenceId::new("00000000000000000000000000000001")?)
            .await?;
        Ok(ResearchRequest {
            scope: "operator".into(),
            request_id: "research-1".into(),
            implementation: "02".repeat(32),
            configuration: "03".repeat(32),
            decision_at_ms: record.request().admitted_at.unix_millis() + 1,
            lookback_sessions: 5,
            instruments: vec![format!("{:032x}", 1)],
        })
    }
    pub async fn once(&self, delivery: &MessageEnvelope) -> Result<MessageEnvelope> {
        MessageEnvelope::from_json(
            &self
                .success(
                    &["offline-research-once", "00000000000000000000000000000001"],
                    Some(&delivery.to_json()?),
                )
                .await?,
        )
        .map_err(Into::into)
    }
    pub async fn finish(
        self,
        result: std::result::Result<Result, Box<dyn std::any::Any + Send>>,
    ) -> Result {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.pool)
        .await?;
        self.pool.close().await;
        std::fs::remove_dir_all(&self.root)?;
        match result {
            Ok(r) => r,
            Err(p) => std::panic::resume_unwind(p),
        }
    }
}
async fn run(
    root: &Path,
    pg: &str,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<std::process::Output> {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_edgeagent-market-data"))
        .args(args)
        .env("USE_MOCK", "1")
        .env("EDGEAGENT_MARKET_DATA_PROVIDER", "synthetic-daily-v1")
        .env("EDGEAGENT_POSTGRES_URL", pg)
        .env("EDGEAGENT_OFFLINE_ROOT", root)
        .env("EDGEAGENT_OFFLINE_RESEARCH_SCOPE", "operator")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(bytes) = input {
        child.stdin.take().ok_or("stdin")?.write_all(bytes).await?;
    }
    // Closing stdin makes a one-record read finite even with no input.
    drop(child.stdin.take());
    Ok(tokio::time::timeout(Duration::from_secs(60), child.wait_with_output()).await??)
}
pub fn delivery(request: &ResearchRequest, id: &str) -> Result<MessageEnvelope> {
    let (key, partition) = research_acquisition::binding(request)?;
    let def = research_acquisition::REQUEST;
    Ok(MessageEnvelope::from_payload(
        MessageMetadata {
            id: id.into(),
            source: Component::Gateway.source_uri().into(),
            message_type: def.message_type().into(),
            subject: partition.clone(),
            time: "2026-10-29T00:00:00Z".into(),
            data_schema: def.data_schema().into(),
            correlation_id: key.clone(),
            causation_id: "cause-1".into(),
            idempotency_key: key,
            partition_key: partition,
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        },
        &ResearchRecord::Request(request.clone()),
    )?)
}
