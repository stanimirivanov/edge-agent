//! Experimental calculation parity and actual command isolation without Research storage.
#[allow(dead_code)]
mod support;
use edgeagent_contracts::{MessageEnvelope, research::ResearchRecord};
use edgeagent_research_application::{EvaluationError, evaluate, experiment};
use support::*;

#[test]
fn experiments_match_normal_calculations_and_reject_incompatible_bindings() -> Result {
    let (event, bytes) = event("experiment-1")?;
    let loaded = edgeagent_research::loader::convert(
        &event,
        &bytes,
        [2; 32],
        [3; 32],
        1_209_600_000,
        60_000,
    )?;
    let request = edgeagent_research_application::publication::evaluation_request(&event)?;
    let normal = evaluate(
        request.clone(),
        &loaded.policy,
        &loaded.series,
        loaded.expires_at_ms,
    )?;
    let experimental = experiment::evaluate(
        request.clone(),
        &loaded.policy,
        &loaded.series,
        loaded.expires_at_ms,
    )?;
    let output: serde_json::Value = serde_json::from_slice(experimental.bytes())?;
    assert_eq!(output["kind"], "research.experiment.v1");
    assert_eq!(output["publication_eligible"], false);
    assert_eq!(
        output["result"],
        serde_json::from_slice::<serde_json::Value>(normal.bytes())?
    );
    assert!(ResearchRecord::decode(experimental.bytes()).is_err());
    assert!(MessageEnvelope::from_json(experimental.bytes()).is_err());
    assert_eq!(
        experimental.bytes(),
        experiment::evaluate(
            request.clone(),
            &loaded.policy,
            &loaded.series,
            loaded.expires_at_ms
        )?
        .bytes()
    );
    let mut incompatible = request.clone();
    incompatible.configuration = "04".repeat(32);
    assert!(matches!(
        experiment::evaluate(
            incompatible,
            &loaded.policy,
            &loaded.series,
            loaded.expires_at_ms
        ),
        Err(EvaluationError::BindingMismatch)
    ));
    assert!(matches!(
        experiment::evaluate(
            request.clone(),
            &loaded.policy,
            &loaded.series,
            request.decision_at_ms
        ),
        Err(EvaluationError::InvalidExpiry)
    ));
    // Evidence-policy abstentions remain evidence failures, never apparent approval.
    let missing = experiment::evaluate(request, &loaded.policy, &[], loaded.expires_at_ms)?;
    let missing: serde_json::Value = serde_json::from_slice(missing.bytes())?;
    assert_eq!(missing["publication_eligible"], false);
    assert_eq!(missing["result"]["kind"], "research.abstention.v1");
    Ok(())
}
async fn run_experiment(
    profile: &Profile,
    input: &[u8],
    configuration: &str,
) -> Result<std::process::Output> {
    use tokio::io::AsyncWriteExt as _;
    let mut command = profile.command("not_a_research_schema", "offline-experiment");
    // Invalid/unreachable Research settings prove experiment composition bypasses storage/broker.
    command
        .env("EDGEAGENT_POSTGRES_URL", "invalid")
        .env("EDGEAGENT_NATS_URL", "invalid")
        .env("EDGEAGENT_RESEARCH_CONFIGURATION", configuration);
    let mut child = tokio::process::Command::from(command)
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
#[tokio::test]
async fn local_experiment_needs_no_research_database_or_approval_and_cannot_publish() -> Result {
    let (event, bytes) = event("experimental-process")?;
    let profile = Profile::new(&bytes)?;
    let input = event.to_json()?;
    let first = run_experiment(&profile, &input, &"03".repeat(32)).await?;
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let output: serde_json::Value = serde_json::from_slice(&first.stdout)?;
    assert_eq!(output["kind"], "research.experiment.v1");
    assert_eq!(output["publication_eligible"], false);
    assert!(ResearchRecord::decode(&first.stdout).is_err());
    assert!(MessageEnvelope::from_json(&first.stdout).is_err());
    assert_eq!(
        first.stdout,
        run_experiment(&profile, &input, &"03".repeat(32))
            .await?
            .stdout
    );
    let mismatched = run_experiment(&profile, &input, &"04".repeat(32)).await?;
    assert!(!mismatched.status.success());
    assert!(mismatched.stdout.is_empty());
    let malformed = run_experiment(&profile, b"PRIVATE_SENTINEL", &"03".repeat(32)).await?;
    assert!(!malformed.status.success());
    assert!(malformed.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&malformed.stderr).contains("PRIVATE_SENTINEL"));
    profile.unavailable()?;
    let unavailable = run_experiment(&profile, &input, &"03".repeat(32)).await?;
    assert!(!unavailable.status.success());
    assert!(unavailable.stdout.is_empty());
    Ok(())
}
