use edgeagent_contracts::research::{AbstentionReason, ResearchRecord, ResearchRequest};
use edgeagent_market_data_domain::{InstrumentId, ListingId, SessionDate, UsdPrice};
use edgeagent_research_application::{
    EvaluationError, RENDERER_REVISION, RenderedOutcome, evaluate,
};
use edgeagent_research_domain::*;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn selection(id: u128) -> Result<SelectedInstrument> {
    Ok(SelectedInstrument {
        instrument: InstrumentId::new(id)?,
        listing: ListingId::new(id)?,
        mapping: [1; 32],
    })
}
fn sessions() -> Result<Vec<SessionDate>> {
    Ok(vec![
        SessionDate::new(2026, 10, 1)?,
        SessionDate::new(2026, 10, 2)?,
    ])
}
fn request() -> ResearchRequest {
    ResearchRequest {
        request_id: "request-01".into(),
        scope: "offline".into(),
        implementation: "02".repeat(32),
        configuration: "03".repeat(32),
        decision_at_ms: 100,
        lookback_sessions: 1,
        instruments: (1..=4).map(|id| format!("{id:032x}")).collect(),
    }
}
fn policy() -> Result<EvidencePolicy> {
    Ok(EvidencePolicy::new(
        PolicyBinding {
            decision_at_ms: 100,
            max_age_ms: 10,
            implementation: [2; 32],
            configuration: [3; 32],
            calendar: [4; 32],
        },
        (1..=4).map(selection).collect::<Result<Vec<_>>>()?,
        sessions()?,
        1,
    )?)
}
fn inputs() -> Result<Vec<FrozenSeries>> {
    (1..=4)
        .map(|id| {
            Ok(FrozenSeries {
                selected: selection(id)?,
                evidence: [id as u8 + 4; 32],
                implementation: [2; 32],
                configuration: [3; 32],
                calendar: [4; 32],
                known_at_ms: 90,
                rights_until_ms: 101,
                entitled: true,
                quality_accepted: true,
                basis: AdjustmentBasis::Raw,
                closes: sessions()?
                    .into_iter()
                    .enumerate()
                    .map(|(i, session)| {
                        Ok(FrozenClose {
                            session,
                            close: UsdPrice::new(
                                if i == 0 {
                                    100
                                } else {
                                    match id {
                                        1 | 2 => 110,
                                        3 => 100,
                                        _ => 90,
                                    }
                                },
                                0,
                            )?,
                            event_at_ms: 80 + i as i64 * 10,
                            observed_at_ms: 90,
                            available_at_ms: 100,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            })
        })
        .collect()
}
fn render(inputs: &[FrozenSeries]) -> Result<RenderedOutcome> {
    Ok(evaluate(request(), &policy()?, inputs, 101)?)
}
fn abstention(out: &RenderedOutcome) -> Result<AbstentionReason> {
    match out.record() {
        ResearchRecord::Abstention(a) => Ok(a.reason),
        _ => Err("expected abstention".into()),
    }
}
#[test]
fn canonical_artifact_and_abstention_match_frozen_bytes() -> Result {
    let out = render(&inputs()?)?;
    assert_eq!(out.bytes(), include_bytes!("fixtures/artifact-v1.json"));
    let failed = render(&[])?;
    assert_eq!(
        failed.bytes(),
        include_bytes!("fixtures/abstention-v1.json")
    );
    let ResearchRecord::Artifact(a) = out.record() else {
        return Err("expected artifact".into());
    };
    assert_eq!(
        a.ranking.iter().map(|r| r.score_ppm).collect::<Vec<_>>(),
        vec![100_000, 100_000, 0, -100_000]
    );
    assert!(a.explanation.starts_with(RENDERER_REVISION));
    Ok(())
}
#[test]
fn permutations_ties_and_session_order_preserve_identical_output() -> Result {
    let base = inputs()?;
    let expected = render(&base)?;
    let mut count = 0;
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    if [a, b, c, d]
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != 4
                    {
                        continue;
                    }
                    let mut reordered = vec![
                        base[a].clone(),
                        base[b].clone(),
                        base[c].clone(),
                        base[d].clone(),
                    ];
                    for row in &mut reordered {
                        row.closes.reverse();
                    }
                    let actual = render(&reordered)?;
                    assert_eq!(actual.bytes(), expected.bytes());
                    assert_eq!(actual.digest(), expected.digest());
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 24);
    Ok(())
}
#[test]
fn mismatched_request_and_policy_bindings_fail_before_rendering() -> Result {
    let p = policy()?;
    let data = inputs()?;
    for change in 0..5 {
        let mut r = request();
        match change {
            0 => r.implementation = "09".repeat(32),
            1 => r.configuration = "09".repeat(32),
            2 => r.decision_at_ms = 99,
            3 => r.lookback_sessions = 2,
            _ => r.instruments.truncate(3),
        };
        assert!(matches!(
            evaluate(r, &p, &data, 101),
            Err(EvaluationError::BindingMismatch)
        ));
    }
    let mut r = request();
    r.instruments.resize(65, "invalid".into());
    assert!(matches!(
        evaluate(r, &p, &data, 101),
        Err(EvaluationError::InvalidRequest)
    ));
    Ok(())
}
#[test]
fn failed_required_evidence_abstains_without_partial_rankings() -> Result {
    let mut data = inputs()?;
    data[0].entitled = false;
    data[1].closes[1].available_at_ms = 101;
    assert_eq!(
        abstention(&render(&data)?)?,
        AbstentionReason::MissingEntitlement
    );
    data.reverse();
    assert_eq!(
        abstention(&render(&data)?)?,
        AbstentionReason::MissingEntitlement
    );
    assert_eq!(
        abstention(&render(&[])?)?,
        AbstentionReason::InsufficientHistory
    );
    let mut data = inputs()?;
    data[0].basis = AdjustmentBasis::SplitAdjusted;
    assert_eq!(
        abstention(&render(&data)?)?,
        AbstentionReason::InvalidEvidence
    );
    Ok(())
}
#[test]
fn expiry_and_numeric_failure_never_publish_an_artifact() -> Result {
    let p = policy()?;
    let mut data = inputs()?;
    for expiry in [100, 102] {
        assert!(matches!(
            evaluate(request(), &p, &data, expiry),
            Err(EvaluationError::InvalidExpiry)
        ));
    }
    assert!(evaluate(request(), &p, &data, 101).is_ok());
    data[0].closes[0].close = UsdPrice::new(1, 8)?;
    data[0].closes[1].close = UsdPrice::new(10_i128.pow(28) - 1, 0)?;
    let error = evaluate(request(), &p, &data, 101)
        .err()
        .ok_or("expected numeric error")?;
    assert!(matches!(error, EvaluationError::Calculation(_)));
    assert_eq!(error.to_string(), "research calculation failed");
    Ok(())
}
#[test]
fn canonical_identity_retains_revision_and_availability_lineage() -> Result {
    let out = render(&inputs()?)?;
    let decoded = ResearchRecord::decode(out.bytes())?;
    assert!(out.record() == &decoded);
    assert_eq!(out.digest(), decoded.digest()?);
    let mut changed = inputs()?;
    changed[0].evidence = [9; 32];
    assert_ne!(render(&changed)?.digest(), out.digest());
    let ResearchRecord::Artifact(a) = out.record() else {
        return Err("expected artifact".into());
    };
    assert!(a.evidence.iter().all(|v| v.available_at_ms == 100));
    assert_eq!(a.request.configuration, "03".repeat(32));
    Ok(())
}
