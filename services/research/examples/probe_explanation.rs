//! Invented capacity calibration, never a qualification collector or worker.
use edgeagent_contracts::explanation::Binding;
use edgeagent_research::{
    explanation_deployment::Verifier,
    explanation_model::HttpModel,
    explanation_owned::{Manifest, digest},
};
use edgeagent_research_application::{
    explanation::Allowlist,
    explanation_evaluation::Suite,
    explanation_model::Request,
    explanation_runtime::{RENDERER_REVISION, VALIDATOR_REVISION},
};
use std::{
    io::Write as _,
    net::{Ipv4Addr, SocketAddrV4},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn run() -> Result<()> {
    let mode = std::env::args().nth(1).ok_or("case required")?;
    let (index, id) = match mode.as_str() {
        "performance-P01" => (1, "P01"),
        "performance-P02" => (2, "P02"),
        "performance-P03" => (3, "P03"),
        _ => return Err("invalid calibration".into()),
    };
    let (manifest, _) = Manifest::environment()?;
    let profile = manifest.profile()?;
    let prompt = manifest.verify(&profile)?;
    if profile.renderer != digest(RENDERER_REVISION.as_bytes())
        || profile.validator != digest(VALIDATOR_REVISION.as_bytes())
    {
        return Err("runtime protocol required".into());
    }
    let observation = manifest.observation()?;
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, observation.port);
    let verifier = Verifier::new(observation, profile.deployment.clone())?;
    let model = HttpModel::new(endpoint, verifier.clone(), prompt)?;
    let started = Instant::now();
    let input = Suite::performance_input(index)?;
    let bytes = input.prepared_input().ok_or("preparation required")?;
    let binding = Binding {
        anchor: Allowlist::from_verified(input.artifact(), input.decision())?
            .standard()?
            .anchor()
            .clone(),
        profile: profile.id()?,
        evidence: digest(bytes),
    };
    let request = Request::new(&profile, &binding, bytes)?;
    let measurement = model
        .measure(&request, &input, Duration::from_secs(180))
        .await;
    verifier.verify()?;
    let report = serde_json::json!({"kind":"research.explanation-performance.attempt.v1","case":id,"profile":profile.id()?,"calibration":digest(bytes),"input_bytes":bytes.len(),"total_ms":started.elapsed().as_millis(),"measurement":measurement});
    std::io::stdout().write_all(&edgeagent_contracts::canonical_json_bytes(&report)?)?;
    Ok(())
}
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("performance probe refused");
        std::process::exit(1);
    }
}
