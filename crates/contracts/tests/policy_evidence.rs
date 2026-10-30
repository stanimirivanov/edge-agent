use edgeagent_contracts::{evidence::*, policy_evidence::*, research::ResearchRequest};
type Result = std::result::Result<(), Box<dyn std::error::Error>>;
fn companion() -> std::result::Result<PolicyEvidence, Box<dyn std::error::Error>> {
    let bytes = include_bytes!("../fixtures/evidence-v1/daily.json");
    let daily = DailyEvidenceArtifact::decode(bytes, &DailyEvidenceArtifact::hash(bytes))?;
    let cutoff = daily.data().captured_at.seconds * 1000;
    let request = ResearchRequest {
        scope: "offline".into(),
        request_id: "policy-1".into(),
        implementation: "02".repeat(32),
        configuration: "03".repeat(32),
        decision_at_ms: cutoff,
        lookback_sessions: 1,
        instruments: vec![daily.data().scope.instrument.as_str().into()],
    };
    Ok(PolicyEvidence::V1 {
        request,
        daily: Box::new(daily),
        events: EventCoverage::Unknown,
    })
}
#[test]
fn canonical_companion_preserves_daily_bytes_and_binding() -> Result {
    let value = companion()?;
    let bytes = value.bytes()?;
    assert!(PolicyEvidence::decode(&bytes)? == value);
    let PolicyEvidence::V1 { request, daily, .. } = &value;
    assert_eq!(
        daily.bytes(),
        include_bytes!("../fixtures/evidence-v1/daily.json")
    );
    value.bind(request, daily.digest())?;
    let mut foreign = request.clone();
    foreign.scope = "foreign".into();
    assert!(value.bind(&foreign, daily.digest()).is_err());
    assert!(
        value
            .bind(request, &DailyEvidenceArtifact::hash(b"wrong"))
            .is_err()
    );
    assert!(PolicyEvidence::decode(&[bytes.as_slice(), b" "].concat()).is_err());
    assert!(PolicyEvidence::decode(&vec![0; MAX_POLICY_BYTES + 1]).is_err());
    Ok(())
}
#[test]
fn coverage_is_explicit_and_future_or_incomplete_coverage_rejects() -> Result {
    let mut value = companion()?;
    let PolicyEvidence::V1 {
        request,
        daily,
        events,
    } = &mut value;
    let cutoff = request.decision_at_ms;
    *events = EventCoverage::SyntheticComplete {
        revision: DailyEvidenceArtifact::hash(b"invented-source"),
        known_at_ms: cutoff,
        interval: [0, daily.data().delete_by.seconds * 1000],
        events: vec![],
    };
    assert!(value.bytes().is_ok());
    for (known, interval, points) in [
        (cutoff + 1, [0, cutoff + 1], vec![]),
        (cutoff, [0, cutoff], vec![]),
        (cutoff, [0, cutoff + 1], vec![1, 1]),
        (cutoff, [0, cutoff + 1], vec![-1]),
    ] {
        let PolicyEvidence::V1 { events, .. } = &mut value;
        *events = EventCoverage::SyntheticComplete {
            revision: DailyEvidenceArtifact::hash(b"invented-source"),
            known_at_ms: known,
            interval,
            events: points,
        };
        assert!(value.bytes().is_err());
    }
    let mut raw: serde_json::Value = serde_json::from_slice(&companion()?.bytes()?)?;
    raw["data"]["events"]["unknown"] = "PRIVATE_SENTINEL".into();
    assert!(PolicyEvidence::decode(&serde_json::to_vec(&raw)?).is_err());
    Ok(())
}
