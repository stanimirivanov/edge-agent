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
    assert_eq!(
        out.digest(),
        "ca63f729179bfb25aff4639d059eb7b321782e82fe5eee9af711d636dbe4e7bb"
    );
    let failed = render(&[])?;
    assert_eq!(
        failed.bytes(),
        include_bytes!("fixtures/abstention-v1.json")
    );
    assert_eq!(
        failed.digest(),
        "1b66c0754e9beb6c64f70501f4a3ebab7f503ac8abb961ca54450a491fc354d3"
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

#[test]
fn registry_preserves_direct_ranking_metadata_and_calculation_errors() -> Result {
    let selected = strategy::resolve(momentum::REVISION).ok_or("missing momentum")?;
    let p = policy()?;
    let mut data = inputs()?;
    for reversed in [false, true] {
        if reversed {
            data.reverse();
            for row in &mut data {
                row.closes.reverse();
            }
        }
        let eligible = p.assess(&data).map_err(|_| "unexpected abstention")?;
        let direct = ranking::rank(&eligible)?;
        let registered = selected.rank(&eligible)?;
        assert_eq!(registered.len(), direct.len());
        for (actual, expected) in registered.iter().zip(&direct) {
            assert_eq!(actual.instrument(), expected.instrument());
            assert!(actual.momentum() == expected.momentum());
            assert_eq!(actual.momentum().revision(), selected.revision());
        }
        let out = render(&data)?;
        let ResearchRecord::Artifact(artifact) = out.record() else {
            return Err("expected artifact".into());
        };
        assert_eq!(artifact.ranking.len(), registered.len());
        for (actual, expected) in artifact.ranking.iter().zip(&registered) {
            assert_eq!(
                actual.instrument,
                format!("{:032x}", expected.instrument().get())
            );
            assert_eq!(actual.score_ppm, expected.momentum().score_ppm());
        }
    }
    let mut data = inputs()?;
    // Fail a later candidate after earlier candidates have calculated successfully.
    data[3].closes[0].close = UsdPrice::new(1, 8)?;
    data[3].closes[1].close = UsdPrice::new(10_i128.pow(28) - 1, 0)?;
    let eligible = p.assess(&data).map_err(|_| "unexpected abstention")?;
    assert_eq!(
        ranking::rank(&eligible).err(),
        Some(momentum::MomentumError::OutOfRange)
    );
    assert_eq!(
        selected.rank(&eligible).err(),
        Some(momentum::MomentumError::OutOfRange)
    );
    assert!(matches!(
        evaluate(request(), &p, &data, 101),
        Err(EvaluationError::Calculation(
            momentum::MomentumError::OutOfRange
        ))
    ));
    Ok(())
}

#[test]
fn governed_eligibility_preserves_scores_and_cannot_hide_failed_evidence() -> Result {
    use edgeagent_research_application::governed;
    use policy::{Direction, Intent, Policy, Status};
    let all = (1..=4)
        .map(InstrumentId::new)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let allowed = Policy::new([8; 32], [3; 32], all, 5)?;
    let intent = Intent {
        direction: Direction::Long,
        horizon_sessions: 5,
    };
    let data = inputs()?;
    let evidence = policy()?;
    let base = render(&data)?;
    let approved = governed::evaluate(request(), &evidence, &data, 101, &allowed, intent)?;
    assert_eq!(approved.outcome().bytes(), base.bytes());
    assert_eq!(
        approved.decision().ok_or("decision")?.status(),
        Status::Approved
    );
    let subset = Policy::new(
        [8; 32],
        [3; 32],
        vec![InstrumentId::new(1)?, InstrumentId::new(3)?],
        5,
    )?;
    let narrowed = governed::evaluate(request(), &evidence, &data, 101, &subset, intent)?;
    assert_eq!(
        narrowed.decision().ok_or("decision")?.status(),
        Status::Narrowed
    );
    let ResearchRecord::Artifact(a) = narrowed.outcome().record() else {
        return Err("artifact".into());
    };
    let ResearchRecord::Artifact(original) = base.record() else {
        return Err("artifact".into());
    };
    assert!(a.request == original.request);
    assert!(a.evidence == original.evidence);
    assert_eq!(
        a.ranking.iter().map(|c| c.score_ppm).collect::<Vec<_>>(),
        vec![100_000, 0]
    );
    assert_eq!(a.expires_at_ms, original.expires_at_ms);
    for denied in [
        Intent {
            direction: Direction::Short,
            horizon_sessions: 5,
        },
        Intent {
            direction: Direction::Long,
            horizon_sessions: 6,
        },
    ] {
        let outcome = governed::evaluate(request(), &evidence, &data, 101, &subset, denied)?;
        assert_eq!(
            abstention(outcome.outcome())?,
            AbstentionReason::NoEligibleCandidate
        );
    }
    let none = Policy::new([8; 32], [3; 32], vec![InstrumentId::new(9)?], 5)?;
    let outcome = governed::evaluate(request(), &evidence, &data, 101, &none, intent)?;
    assert_eq!(
        abstention(outcome.outcome())?,
        AbstentionReason::NoEligibleCandidate
    );
    let mut invalid = data;
    invalid[1].entitled = false;
    let outcome = governed::evaluate(request(), &evidence, &invalid, 101, &subset, intent)?;
    assert_eq!(
        abstention(outcome.outcome())?,
        AbstentionReason::MissingEntitlement
    );
    assert!(outcome.decision().is_none());
    let foreign = Policy::new([8; 32], [9; 32], vec![InstrumentId::new(1)?], 5)?;
    assert!(matches!(
        governed::evaluate(request(), &evidence, &invalid, 101, &foreign, intent),
        Err(governed::Error::Binding)
    ));
    Ok(())
}

#[test]
fn liquidity_is_required_and_narrows_without_rescoring_or_hiding_missing_rows() -> Result {
    use edgeagent_market_data_domain::ShareVolume;
    use edgeagent_research_application::governed;
    use liquidity::{Input, REVISION, Volume};
    use policy::{Direction, Intent, Policy, Reason};
    let make_policy = |minimum| -> Result<Policy> {
        Ok(Policy::new(
            [8; 32],
            [3; 32],
            (1..=4)
                .map(InstrumentId::new)
                .collect::<std::result::Result<_, _>>()?,
            5,
        )?
        .with_liquidity(REVISION, minimum)?)
    };
    let data = inputs()?;
    let evidence = policy()?;
    let intent = Intent {
        direction: Direction::Long,
        horizon_sessions: 1,
    };
    let mut volumes = data
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Ok(Input {
                instrument: s.selected.instrument,
                evidence: s.evidence,
                rows: s
                    .closes
                    .iter()
                    .map(|c| {
                        Ok(Volume {
                            session: c.session,
                            shares: ShareVolume::new(if i == 0 || i == 2 { 100 } else { 0 }, 0)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let zero = make_policy(0)?;
    let allowed = governed::evaluate_with_liquidity(
        request(),
        &evidence,
        &data,
        101,
        &zero,
        intent,
        &volumes,
    )?;
    assert_eq!(allowed.outcome().bytes(), render(&data)?.bytes());
    let required = governed::evaluate(request(), &evidence, &data, 101, &zero, intent)?;
    assert_eq!(
        abstention(required.outcome())?,
        AbstentionReason::InsufficientHistory
    );
    let gate = make_policy(1_000_000)?;
    let narrowed = governed::evaluate_with_liquidity(
        request(),
        &evidence,
        &data,
        101,
        &gate,
        intent,
        &volumes,
    )?;
    assert_eq!(
        narrowed.decision().ok_or("decision")?.reasons(),
        [Reason::LiquidityBelowMinimum]
    );
    let ResearchRecord::Artifact(a) = narrowed.outcome().record() else {
        return Err("artifact".into());
    };
    assert_eq!(
        a.ranking.iter().map(|c| c.score_ppm).collect::<Vec<_>>(),
        vec![100_000, 0]
    );
    let ResearchRecord::Artifact(original) = render(&data)?.record().clone() else {
        return Err("artifact".into());
    };
    assert!(a.evidence == original.evidence);
    assert!(a.request == original.request);
    let all_denied = make_policy(u64::MAX)?;
    let denied = governed::evaluate_with_liquidity(
        request(),
        &evidence,
        &data,
        101,
        &all_denied,
        intent,
        &volumes,
    )?;
    assert_eq!(
        abstention(denied.outcome())?,
        AbstentionReason::NoEligibleCandidate
    );
    volumes[1].rows.pop();
    let missing = governed::evaluate_with_liquidity(
        request(),
        &evidence,
        &data,
        101,
        &gate,
        intent,
        &volumes,
    )?;
    assert_eq!(
        abstention(missing.outcome())?,
        AbstentionReason::InsufficientHistory
    );
    assert!(missing.decision().is_none());
    assert!(
        Policy::new([8; 32], [3; 32], vec![InstrumentId::new(1)?], 1)?
            .with_liquidity("unknown", 0)
            .is_err()
    );
    Ok(())
}

#[test]
fn volatility_and_gap_limits_are_inclusive_and_cannot_hide_unsafe_or_missing_evidence() -> Result {
    use edgeagent_research_application::governed::{self, ConstraintInputs};
    use market_constraints::{GAP_REVISION, Input, Open, VOLATILITY_REVISION};
    use policy::{Direction, Intent, Policy, Reason};
    let make_policy = |vol, gap| -> Result<Policy> {
        Ok(Policy::new(
            [8; 32],
            [3; 32],
            (1..=4)
                .map(InstrumentId::new)
                .collect::<std::result::Result<_, _>>()?,
            5,
        )?
        .with_market_constraints(VOLATILITY_REVISION, GAP_REVISION, vol, gap)?)
    };
    let mut data = inputs()?;
    for s in &mut data {
        let mut middle = s.closes[0].clone();
        middle.session = SessionDate::new(2026, 10, 2)?;
        middle.event_at_ms = 85;
        s.closes[1].session = SessionDate::new(2026, 10, 3)?;
        s.closes.insert(1, middle);
    }
    let p = policy()?;
    let evidence = EvidencePolicy::new(
        p.binding(),
        p.universe().to_vec(),
        (1..=3)
            .map(|d| SessionDate::new(2026, 10, d))
            .collect::<std::result::Result<_, _>>()?,
        2,
    )?;
    let mut req = request();
    req.lookback_sessions = 2;
    let mut opens = data
        .iter()
        .map(|s| Input {
            instrument: s.selected.instrument,
            evidence: s.evidence,
            rows: s
                .closes
                .iter()
                .map(|c| Open {
                    session: c.session,
                    price: c.close,
                    action: false,
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let intent = Intent {
        direction: Direction::Long,
        horizon_sessions: 1,
    };
    let gate = make_policy(50_000, 100_000)?;
    let allowed = governed::evaluate_with_market_inputs(
        req.clone(),
        &evidence,
        &data,
        101,
        &gate,
        intent,
        ConstraintInputs {
            volumes: &[],
            opens: &opens,
        },
    )?;
    assert_eq!(
        allowed.outcome().bytes(),
        evaluate(req.clone(), &evidence, &data, 101)?.bytes()
    );
    for gate in [
        make_policy(49_999, 100_000)?,
        make_policy(50_000, 99_999)?,
        make_policy(0, 0)?,
    ] {
        let narrowed = governed::evaluate_with_market_inputs(
            req.clone(),
            &evidence,
            &data,
            101,
            &gate,
            intent,
            ConstraintInputs {
                volumes: &[],
                opens: &opens,
            },
        )?;
        let ResearchRecord::Artifact(a) = narrowed.outcome().record() else {
            return Err("artifact".into());
        };
        assert_eq!(a.ranking.len(), 1);
        assert_eq!(a.ranking[0].instrument, format!("{:032x}", 3));
        assert_eq!(a.ranking[0].score_ppm, 0);
        assert_eq!(a.evidence.len(), 4);
        assert!(a.request == req);
    }
    let combined = make_policy(49_999, 99_999)?;
    let narrowed = governed::evaluate_with_market_inputs(
        req.clone(),
        &evidence,
        &data,
        101,
        &combined,
        intent,
        ConstraintInputs {
            volumes: &[],
            opens: &opens,
        },
    )?;
    assert_eq!(
        narrowed.decision().ok_or("decision")?.reasons(),
        [Reason::VolatilityExceeded, Reason::GapExceeded]
    );
    let absent = governed::evaluate(req.clone(), &evidence, &data, 101, &gate, intent)?;
    assert_eq!(
        abstention(absent.outcome())?,
        AbstentionReason::InsufficientHistory
    );
    let refusal = |opens: &[Input]| -> Result<AbstentionReason> {
        let out = governed::evaluate_with_market_inputs(
            req.clone(),
            &evidence,
            &data,
            101,
            &combined,
            intent,
            ConstraintInputs {
                volumes: &[],
                opens,
            },
        )?;
        abstention(out.outcome())
    };
    opens[0].evidence = [9; 32];
    assert_eq!(refusal(&opens)?, AbstentionReason::InvalidEvidence);
    opens[0].evidence = data[0].evidence;
    let second = opens[0].rows[1].session;
    opens[0].rows[1].session = opens[0].rows[0].session;
    assert_eq!(refusal(&opens)?, AbstentionReason::InvalidEvidence);
    opens[0].rows[1].session = second;
    opens[0].rows[0].action = true;
    let unsafe_action = governed::evaluate_with_market_inputs(
        req.clone(),
        &evidence,
        &data,
        101,
        &gate,
        intent,
        ConstraintInputs {
            volumes: &[],
            opens: &opens,
        },
    )?;
    assert_eq!(
        abstention(unsafe_action.outcome())?,
        AbstentionReason::InvalidEvidence
    );
    assert!(unsafe_action.decision().is_none());
    opens[0].rows[0].action = false;
    opens[1].rows.pop();
    let missing = governed::evaluate_with_market_inputs(
        req.clone(),
        &evidence,
        &data,
        101,
        &gate,
        intent,
        ConstraintInputs {
            volumes: &[],
            opens: &opens,
        },
    )?;
    assert_eq!(
        abstention(missing.outcome())?,
        AbstentionReason::InsufficientHistory
    );
    assert!(
        Policy::new([8; 32], [3; 32], vec![InstrumentId::new(1)?], 1)?
            .with_market_constraints("unknown", GAP_REVISION, 0, 0)
            .is_err()
    );
    Ok(())
}
