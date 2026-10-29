//! Pure conversion/rejection and unchanged descriptor behavior.
#[allow(dead_code)]
mod support;
use support::*;
#[test]
fn verified_bytes_produce_deterministic_outcomes_and_wrong_digest_never_ranks() -> Result {
    let (event, bytes) = event("request-1")?;
    let loaded = edgeagent_research::loader::convert(
        &event,
        &bytes,
        [2; 32],
        [3; 32],
        1_209_600_000,
        60_000,
    )
    .map_err(|_| "conversion")?;
    let request = edgeagent_research_application::publication::evaluation_request(&event)?;
    let first = edgeagent_research_application::evaluate(
        request.clone(),
        &loaded.policy,
        &loaded.series,
        loaded.expires_at_ms,
    )?;
    assert!(matches!(
        first.record(),
        edgeagent_contracts::research::ResearchRecord::Artifact(_)
    ));
    assert_eq!(
        first.bytes(),
        edgeagent_research_application::evaluate(
            request,
            &loaded.policy,
            &loaded.series,
            loaded.expires_at_ms
        )?
        .bytes()
    );
    let mut corrupt = bytes;
    corrupt.push(b' ');
    assert!(
        edgeagent_research::loader::convert(
            &event,
            &corrupt,
            [2; 32],
            [3; 32],
            1_209_600_000,
            60_000
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn runtime_rejects_nonlocal_profiles_and_preserves_descriptor_commands() -> Result {
    for endpoint in [
        "postgresql://remote.invalid/db",
        "postgresql://127.0.0.1/db?host=remote.invalid",
    ] {
        assert!(edgeagent_research::runtime::local(endpoint, &["postgresql"]).is_err());
    }
    for mode in ["describe", "self-check", "--version", "--help"] {
        assert!(
            std::process::Command::new(env!("CARGO_BIN_EXE_edgeagent-research"))
                .arg(mode)
                .status()?
                .success()
        );
    }
    Ok(())
}
