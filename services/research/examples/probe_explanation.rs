//! Invented capacity calibration, never a qualification collector or worker.
use edgeagent_contracts::explanation::Binding;
use edgeagent_research::{
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
fn calibration(mode: &str) -> Result<(usize, &'static str, Duration)> {
    let (index, case, seconds) = match mode {
        "performance-P01" => (1, "P01", 180),
        "performance-P02" => (2, "P02", 180),
        "performance-P03" => (3, "P03", 180),
        "performance-P03-extended" => (3, "P03", 600),
        _ => return Err("invalid calibration".into()),
    };
    Ok((index, case, Duration::from_secs(seconds)))
}

fn p03_tokenizer_reference() -> Result<Vec<u8>> {
    let input = Suite::performance_input(3)?;
    let bytes = input.standard_proposal();
    input.render_candidate(bytes)?;
    Ok(bytes.to_vec())
}

async fn run() -> Result<()> {
    let mode = std::env::args().nth(1).ok_or("case required")?;
    if mode == "export-p03-tokenizer-reference" {
        std::io::stdout().write_all(&p03_tokenizer_reference()?)?;
        return Ok(());
    }
    let (index, id, observation_limit) = calibration(&mode)?;
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
    let verifier = manifest.verifier(observation, profile.deployment.clone())?;
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
    let measurement = model.measure(&request, &input, observation_limit).await;
    verifier.verify()?;
    let report = serde_json::json!({"kind":"research.explanation-performance.attempt.v3","case":id,"profile":profile.id()?,"calibration":digest(bytes),"input_bytes":bytes.len(),"total_ms":started.elapsed().as_millis(),"measurement":measurement});
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_reference_is_the_validated_p03_proposal() -> Result<()> {
        let bytes = p03_tokenizer_reference()?;
        let input = Suite::performance_input(3)?;
        assert_eq!(bytes, input.standard_proposal());
        assert!(!input.render_candidate(&bytes)?.is_empty());
        assert!(edgeagent_contracts::explanation::Proposal::decode(&bytes).is_ok());
        Ok(())
    }

    #[test]
    fn only_extended_p03_gets_the_longer_observation_window() -> Result<()> {
        assert_eq!(
            calibration("performance-P03-extended")?,
            (3, "P03", Duration::from_secs(600))
        );
        for (index, case) in [(1, "P01"), (2, "P02"), (3, "P03")] {
            assert_eq!(
                calibration(&format!("performance-{case}"))?,
                (index, case, Duration::from_secs(180))
            );
        }
        assert!(calibration("performance-P01-extended").is_err());
        Ok(())
    }
}
