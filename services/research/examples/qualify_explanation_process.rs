//! Synthetic integration executable, deliberately separate from the Research worker.
//! The owned fixture asserts model-disclosure rights for invented data only.
use edgeagent_contracts::{
    explanation_profile::{Deployment, Profile},
    research::ResearchRecord,
};
use edgeagent_research::{
    explanation_model::{DeploymentLease, DeploymentVerifier, HttpModel},
    loader::OwnerLoader,
};
use edgeagent_research_application::{
    explanation_context::{Purpose, Request},
    explanation_model::Failure,
    explanation_worker::Prepared,
    publication::decode_decision,
    replay::verify_execution,
};
use edgeagent_research_postgres::{
    PostgresResearchStore,
    explanation_attempts::{Intent, ReservationOutcome},
    explanation_completion::Completion,
};
use std::{
    io::{Read as _, Write as _},
    net::SocketAddrV4,
    time::Duration,
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const PROMPT: &str = "Select only the supplied synthetic claims.";
const AUDIENCE: &str = "00000000000000000000000000000002";
struct Fixture(SocketAddrV4, Deployment);
impl DeploymentLease for Fixture {
    fn endpoint(&self) -> SocketAddrV4 {
        self.0
    }
    fn identity(&self) -> &Deployment {
        &self.1
    }
}
impl DeploymentVerifier for Fixture {
    type Lease = Self;
    async fn acquire(&self, endpoint: SocketAddrV4) -> std::result::Result<Self, Failure> {
        if endpoint != self.0 {
            return Err(Failure::DeploymentMismatch);
        }
        // Identity describes only the synthetic server owned by the test driver.
        Ok(Self(self.0, self.1.clone()))
    }
}
fn required(name: &str) -> Result<String> {
    Ok(std::env::var(name)?)
}
fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
// Optional observation barriers belong only to this explicitly opted-in fixture.
async fn checkpoint(root: &std::path::Path, stage: &str) -> Result {
    if std::env::var("EDGEAGENT_SYNTHETIC_EXPLANATION_BARRIER")
        .ok()
        .as_deref()
        == Some(stage)
    {
        std::fs::write(root.join(format!("proof-{stage}")), b"ready")?;
        while !root.join(format!("proof-release-{stage}")).is_file() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    Ok(())
}
async fn run() -> Result<Vec<u8>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if required("EDGEAGENT_SYNTHETIC_EXPLANATION_PROOF")? != "1"
        || args.len() != 1
        || !matches!(args[0].as_str(), "valid" | "injection")
        || required("USE_MOCK")? != "1"
        || required("EDGEAGENT_MARKET_DATA_PROVIDER")? != "synthetic-daily-v1"
        || required("EDGEAGENT_RESEARCH_SCOPE")? != "operator"
    {
        return Err("synthetic proof configuration refused".into());
    }
    let root = std::path::PathBuf::from(required("EDGEAGENT_RESEARCH_OWNER_ROOT")?);
    if !root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("research-processes-"))
        || std::fs::read(root.join("synthetic-explanation-proof"))?
            != b"PR19 invented fixtures only\n"
    {
        return Err("invented fixture marker required".into());
    }
    let database = required("EDGEAGENT_POSTGRES_URL")?;
    edgeagent_research::runtime::local(&database, &["postgres", "postgresql"])?;
    let schema = required("EDGEAGENT_RESEARCH_SCHEMA")?;
    if !schema.starts_with("research_")
        || schema.len() > 63
        || !schema
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("invalid fixture schema".into());
    }
    let options: sqlx::postgres::PgConnectOptions = database.parse()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options.options([
            ("search_path", schema.as_str()),
            ("statement_timeout", "15000"),
            ("lock_timeout", "5000"),
        ]))
        .await?;
    let store = PostgresResearchStore::new(pool.clone());
    let job = store
        .explanation_job(
            "operator",
            Some(&required("EDGEAGENT_RESEARCH_EXPLANATION_JOB")?),
        )
        .await?
        .ok_or("job")?;
    if job.receipt.is_some() || job.request.request_id != "qualification" {
        return Err("fresh synthetic job required".into());
    }
    let input = store.explanation_input(&job).await?;
    let original = store.retained_for_replay(&input).await?.ok_or("original")?;
    let selection = original.selection.ok_or("selection")?;
    let loader = OwnerLoader::from_environment()?;
    verify_execution(
        &loader,
        &input,
        &original.receipt,
        selection,
        edgeagent_research_application::RENDERER_REVISION,
        edgeagent_research_domain::momentum::REVISION,
    )
    .await?;
    let decision = decode_decision(original.receipt.decision().ok_or("decision")?)?;
    let mut bytes = Vec::new();
    std::fs::File::open(required("EDGEAGENT_RESEARCH_EXPLANATION_PROFILE")?)?
        .take(2049)
        .read_to_end(&mut bytes)?;
    let profile = Profile::decode(&bytes)?;
    if profile.deployment.runtime != "a".repeat(64) || profile.deployment.weights != "b".repeat(64)
    {
        return Err("only the invented deployment is supported".into());
    }
    let prepared = Prepared::new(original.receipt.record(), &decision, &profile)?;
    let request = Request {
        anchor: job.job.anchor.clone(),
        audience: AUDIENCE.into(),
        purpose: Purpose::ModelDisclosure,
        document: None,
    };
    let grant = store
        .explanation_grant(&request, now())
        .await?
        .ok_or("model grant")?;
    let intent = Intent {
        binding: prepared.binding().clone(),
        input_digest: prepared.input_digest(),
        audience: AUDIENCE.into(),
        deadline_ms: now() + 5000,
    };
    let ReservationOutcome::Reserved(permit) =
        store.reserve_explanation_attempt(&intent, now).await?
    else {
        return Err("attempt spent".into());
    };
    checkpoint(&root, "reserved").await?;
    let endpoint: SocketAddrV4 =
        std::fs::read_to_string(root.join("synthetic-model-endpoint"))?.parse()?;
    if *endpoint.ip() != std::net::Ipv4Addr::LOCALHOST || endpoint.port() == 0 {
        return Err("synthetic server must use IPv4 loopback".into());
    }
    let model = HttpModel::new(
        endpoint,
        Fixture(endpoint, profile.deployment.clone()),
        PROMPT.into(),
    )?;
    let resolved = prepared.infer(&profile, &model).await?;
    std::fs::write(
        root.join("proof-model-result"),
        if resolved.reason.is_none() {
            b"generated".as_slice()
        } else {
            b"fallback".as_slice()
        },
    )?;
    checkpoint(&root, "response").await?;
    let serving = Request {
        purpose: Purpose::ExplanationServing,
        ..grant.request
    };
    let fixture_owner = store
        .explanation_grant(&serving, now())
        .await?
        .ok_or("serving fixture grant")?;
    // Only the owned invented fixture supplies this owner assertion. This is
    // not production evidence-owner model-disclosure qualification. Actual
    // MarketData/Research serving authorization is checked by the query journey.
    let completion = match resolved.reason {
        Some(reason) => Completion::Fallback {
            explanation: &resolved.explanation,
            reason,
        },
        None => Completion::Generated {
            explanation: &resolved.explanation,
            permit: &permit,
        },
    };
    let time = chrono::Utc::now().to_rfc3339();
    let receipt = store
        .complete_explanation(
            &job.request,
            prepared.binding(),
            completion,
            Some(&fixture_owner),
            now,
            &time,
        )
        .await?;
    checkpoint(&root, "committed").await?;
    let ResearchRecord::Artifact(_) = original.receipt.record() else {
        return Err("artifact".into());
    };
    let result = receipt.bytes().to_vec();
    pool.close().await;
    Ok(result)
}
#[tokio::main]
async fn main() {
    if let Ok(Ok(bytes)) = tokio::time::timeout(Duration::from_secs(25), run()).await
        && std::io::stdout().write_all(&bytes).is_ok()
    {
        return;
    }
    eprintln!("synthetic explanation qualification refused");
    std::process::exit(2);
}
