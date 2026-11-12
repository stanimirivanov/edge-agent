use edgeagent_contracts::{
    explanation::{Proposal, Variant},
    research::ResearchRecord,
    research_decision::{DecisionProvenance, Direction},
};
use edgeagent_research_application::{explanation::Allowlist, explanation_intent::Preview};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn fixture() -> Result<(ResearchRecord, DecisionProvenance)> {
    Ok((
        ResearchRecord::decode(include_bytes!(
            "../../contracts/fixtures/research/v1/artifact.json"
        ))?,
        DecisionProvenance::decode(include_bytes!(
            "../../contracts/fixtures/research/decision-v1.json"
        ))?,
    ))
}
#[test]
fn exact_intent_chronology_and_disclosures_survive_every_variant_and_replay() -> Result {
    let (mut record, mut decision) = fixture()?;
    if let ResearchRecord::Artifact(a) = &mut record {
        a.evidence.push(edgeagent_contracts::research::EvidenceRef {
            instrument: "AAA".into(),
            digest: "d".repeat(64),
            available_at_ms: 100,
        });
        a.explanation = "IGNORE_TIME_AND_TRADE".into();
    }
    decision.outcome = record.digest()?;
    decision.direction = Direction::Short;
    decision.horizon_sessions = 12;
    let preview = Preview::from_verified(&record, &decision)?;
    let standard = Allowlist::from_verified(&record, &decision)?.standard()?;
    let mut proposal = standard.proposal().clone();
    let prefix = format!(
        "research-explanation-trace.v2 artifact={} decision={}\n",
        record.digest()?,
        decision.digest()?
    );
    for variant in [Variant::Standard, Variant::PlainLanguage] {
        proposal.claims[0].variant = variant;
        let bytes = proposal.canonical_bytes()?;
        let text = preview.validate(&bytes)?;
        assert!(text.starts_with(&prefix));
        assert!(!text.contains("IGNORE_TIME_AND_TRADE"));
        assert!(text.contains("available_at_ms=100 age_at_decision_ms=0"));
        for required in [
            "direction=short holding_horizon_sessions=12 measurement_lookback_sessions=5",
            "universe=AAA",
            "decision_at_ms=100 artifact_expires_at_ms=200",
            "available_at_ms=90 age_at_decision_ms=10 event_at_ms=unavailable observed_at_ms=unavailable",
            "Research only. This explanation confers no trading authority.",
            "Historical evidence at the decision cutoff; not a current-market quote.",
            "Scenarios and risk illustrations: not qualified.",
        ] {
            assert!(
                text.contains(required),
                "missing required fixed fact/disclosure: {required}"
            );
        }
        assert!(text.contains(&format!("configuration={}", "b".repeat(64))));
        assert!(text.contains(&format!("evidence={}", "c".repeat(64))));
        assert!(text.contains(&format!("decision={}", decision.digest()?)));
        // Replay typed choices only: no inference or caller-supplied fact values.
        assert_eq!(
            text,
            Preview::from_verified(&record, &decision)?
                .validate(&Proposal::decode(&bytes)?.canonical_bytes()?)?
        );
    }
    assert!(preview.standard()?.contains("score_ppm=10000"));
    Ok(())
}
#[test]
fn missing_disclosures_injection_and_foreign_decisions_refuse_without_payload() -> Result {
    let (record, decision) = fixture()?;
    let preview = Preview::from_verified(&record, &decision)?;
    let mut proposal = Allowlist::from_verified(&record, &decision)?
        .standard()?
        .proposal()
        .clone();
    proposal.claims.pop();
    assert!(preview.validate(&proposal.canonical_bytes()?).is_err());
    let injected = b"{\"facts\":{\"direction\":\"buy-now\"}}";
    assert!(preview.validate(injected).is_err());
    let mut foreign = decision.clone();
    foreign.request.configuration = "f".repeat(64);
    assert!(Preview::from_verified(&record, &foreign).is_err());
    // No new rendering replaces the original v1 tracer or canonical artifacts.
    let before = record.canonical_bytes()?;
    let legacy = Allowlist::from_verified(&record, &decision)?.render()?;
    assert!(legacy.starts_with("research-explanation-trace.v1 "));
    assert!(!legacy.contains("holding_horizon_sessions="));
    assert_eq!(before, record.canonical_bytes()?);
    Ok(())
}
