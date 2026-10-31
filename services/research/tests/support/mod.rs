use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    evidence::*,
    research::ResearchRequest,
    research_acquisition::{self, ResearchEvidence},
};
use std::sync::atomic::{AtomicUsize, Ordering};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub fn event(id: &str) -> Result<(MessageEnvelope, Vec<u8>)> {
    event_with_configuration(id, "03".repeat(32))
}
pub fn governed_event(id: &str) -> Result<(MessageEnvelope, Vec<u8>)> {
    event_with_configuration(id, execution()?.digest()?)
}
pub fn execution() -> Result<edgeagent_contracts::strategy::ExecutionConfiguration> {
    let mut value = edgeagent_contracts::strategy::ExecutionConfiguration::decode(include_bytes!(
        "../../../../crates/contracts/fixtures/strategy/execution-v1.json"
    ))?;
    let edgeagent_contracts::strategy::ExecutionConfiguration::V1 {
        bundle,
        context_max_age_ms,
        ..
    } = &mut value;
    let edgeagent_contracts::strategy::Configuration::V1 {
        strategy, policy, ..
    } = bundle;
    strategy.expiry_ms = 60_000;
    policy.instruments = vec!["11111111111111111111111111111111".into()];
    strategy.benchmark = policy.instruments[0].clone();
    policy.max_age_ms = 1_209_600_000;
    *context_max_age_ms = 1_209_600_000;
    Ok(value)
}
pub fn governance_profile() -> Result<edgeagent_contracts::strategy_governance::Profile> {
    Ok(
        edgeagent_research_application::execution::Selection::decode(
            &execution()?.canonical_bytes()?,
        )?
        .profile("operator"),
    )
}
fn event_with_configuration(id: &str, configuration: String) -> Result<(MessageEnvelope, Vec<u8>)> {
    let mut value: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../crates/contracts/fixtures/evidence-v1/daily.json"
    ))?;
    let mut bar = value["bars"][0].clone();
    bar["date"] = "1970-01-02".into();
    bar["calendar"]["id"] = "00000000000000000000000000000002".into();
    bar["observation"]["id"] = "00000000000000000000000000000002".into();
    bar["bucket"][0]["seconds"] = 86400.into();
    bar["bucket"][1]["seconds"] = 172800.into();
    bar["event_at"]["seconds"] = 172800.into();
    value["bars"].as_array_mut().ok_or("bars")?.push(bar);
    value["range"][1] = "1970-01-03".into();
    let mut calendar = value["references"]
        .as_array()
        .ok_or("references")?
        .iter()
        .find(|v| v["kind"] == "calendar")
        .ok_or("calendar")?
        .clone();
    calendar["version"]["id"] = "00000000000000000000000000000002".into();
    calendar["date"] = "1970-01-02".into();
    value["references"]
        .as_array_mut()
        .ok_or("references")?
        .push(calendar);
    value["cutoff"]["seconds"] = 259200.into();
    value["captured_at"]["seconds"] = 259200.into();
    value["delete_by"]["seconds"] = 345600.into();
    value["quality"]["reference_age_ms"] = "345600000".into();
    let seconds = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    )?;
    let shift = (seconds / 86400 * 86400) - 259200;
    fn move_times(value: &mut serde_json::Value, shift: i64) -> Result {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(seconds) = fields.get_mut("seconds") {
                    *seconds = (seconds.as_i64().ok_or("seconds")? + shift).into();
                }
                for value in fields.values_mut() {
                    move_times(value, shift)?;
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    move_times(value, shift)?;
                }
            }
            serde_json::Value::String(text) if text.len() == 10 && text.as_bytes()[4] == b'-' => {
                let date = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")?
                    .checked_add_signed(chrono::TimeDelta::seconds(shift))
                    .ok_or("date")?;
                *text = date.format("%Y-%m-%d").to_string();
            }
            _ => (),
        }
        Ok(())
    }
    move_times(&mut value, shift)?;
    value["captured_at"]["seconds"] = seconds.into();
    value["cutoff"]["seconds"] = seconds.into();
    value["delete_by"]["seconds"] = (seconds + 86400).into();
    value["quality"]["reference_age_ms"] = "432000000".into();
    let artifact = DailyEvidenceArtifact::new(serde_json::from_value(value)?)?;
    let request = ResearchRequest {
        scope: "operator".into(),
        request_id: id.into(),
        implementation: "02".repeat(32),
        configuration,
        decision_at_ms: artifact.data().captured_at.seconds * 1000 + 1,
        lookback_sessions: 1,
        instruments: vec![artifact.data().scope.instrument.as_str().into()],
    };
    let decision = EvidenceDecision::ready(
        AcquireEvidence {
            request: artifact.data().scope.request.clone(),
            request_sha256: artifact.data().scope.request_sha256.clone(),
        },
        &artifact,
    )?;
    let outcome = ResearchEvidence::Ready {
        request: request.clone(),
        decision: Box::new(decision.data().clone()),
    };
    let (key, partition) = research_acquisition::binding(&request)?;
    let def = research_acquisition::DECIDED;
    let envelope = MessageEnvelope::from_payload(
        MessageMetadata {
            id: format!("evidence-{key}"),
            source: Component::MarketData.source_uri().into(),
            message_type: def.message_type().into(),
            subject: partition.clone(),
            time: "2026-10-29T00:00:00Z".into(),
            data_schema: def.data_schema().into(),
            correlation_id: key.clone(),
            causation_id: "acquisition-1".into(),
            idempotency_key: key,
            partition_key: partition,
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        },
        &outcome,
    )?;
    Ok((envelope, artifact.bytes().to_vec()))
}
pub struct Profile {
    pub root: std::path::PathBuf,
    pub owner: std::path::PathBuf,
    pub artifact: std::path::PathBuf,
}
impl Profile {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt as _;
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "research-worker-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&root)?;
        let artifact = root.join("artifact.json");
        std::fs::write(&artifact, bytes)?;
        std::fs::write(root.join("execution.json"), execution()?.canonical_bytes()?)?;
        std::fs::write(
            root.join("policy.py"),
            include_str!("../fixtures/owner-policy.py"),
        )?;
        let owner = root.join("owner");
        std::fs::write(
            &owner,
            format!(
                "#!/bin/sh\nif [ \"$1\" = offline-research-policy-read ]; then exec /usr/bin/python3 '{}' '{}'; fi\nexec /bin/cat '{}'\n",
                root.join("policy.py").display(),
                artifact.display(),
                artifact.display()
            ),
        )?;
        std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            root,
            owner,
            artifact,
        })
    }
    pub fn command(&self, schema: &str, mode: &str) -> std::process::Command {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_edgeagent-research"));
        command
            .arg(mode)
            .env(
                "EDGEAGENT_POSTGRES_URL",
                std::env::var("EDGEAGENT_POSTGRES_URL").unwrap_or_default(),
            )
            .env("EDGEAGENT_RESEARCH_SCHEMA", schema)
            .env("EDGEAGENT_RESEARCH_SCOPE", "operator")
            .env("EDGEAGENT_RESEARCH_OWNER_BINARY", &self.owner)
            .env("EDGEAGENT_RESEARCH_OWNER_ROOT", &self.root)
            .env(
                "EDGEAGENT_RESEARCH_OWNER_DATABASE_URL",
                "postgresql://test@127.0.0.1/test",
            )
            .env(
                "EDGEAGENT_RESEARCH_OWNER_ACQUISITION",
                "11111111111111111111111111111111",
            )
            .env("EDGEAGENT_RESEARCH_IMPLEMENTATION", "02".repeat(32))
            .env(
                "EDGEAGENT_RESEARCH_CONFIGURATION",
                execution()
                    .and_then(|v| Ok(v.digest()?))
                    .unwrap_or_default(),
            )
            .env(
                "EDGEAGENT_RESEARCH_EXECUTION_CONFIGURATION",
                self.root.join("execution.json"),
            )
            .env("EDGEAGENT_RESEARCH_MAX_AGE_MS", "1209600000")
            .env("EDGEAGENT_RESEARCH_TTL_MS", "60000");
        command
    }
    pub fn unavailable(&self) -> Result {
        std::fs::write(&self.owner, "#!/bin/sh\nexit 75\n")?;
        Ok(())
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
pub async fn approve(schema: &str) -> Result {
    let url = std::env::var("EDGEAGENT_POSTGRES_URL")?;
    let options: sqlx::postgres::PgConnectOptions = url.parse()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.options([("search_path", schema)]))
        .await?;
    let store = edgeagent_research_postgres::PostgresResearchStore::new(pool.clone());
    let binding = governance_profile()?;
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM edgeagent_research_approvals WHERE caller_scope=$1 AND configuration=$2)").bind(&binding.scope).bind(&binding.configuration).fetch_one(&pool).await?;
    if !exists {
        store.govern(&binding, 1).await?;
        store.govern(&binding, 2).await?;
    }
    pool.close().await;
    Ok(())
}
pub async fn run(profile: &Profile, schema: &str, input: &[u8]) -> Result<std::process::Output> {
    approve(schema).await?;
    use tokio::io::AsyncWriteExt as _;
    let mut command = tokio::process::Command::from(profile.command(schema, "offline-once"));
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    child.stdin.take().ok_or("stdin")?.write_all(input).await?;
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
            .await??,
    )
}
