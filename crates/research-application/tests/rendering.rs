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

#[test]
fn context_gates_use_exact_regimes_closed_windows_and_critical_freshness() -> Result {
    use context_policy::{
        Coverage, EVENT_REVISION, Input, QUALITY_REVISION, REGIME_REVISION, Regime, Rules,
    };
    use edgeagent_research_application::governed::{self, ConstraintInputs, ContextInputs};
    use policy::{Direction, Intent, Policy, Reason};
    let data = inputs()?;
    let evidence = policy()?;
    let intent = Intent {
        direction: Direction::Long,
        horizon_sessions: 1,
    };
    let gate = |before, after, age, allowed| -> Result<Policy> {
        Ok(Policy::new(
            [8; 32],
            [3; 32],
            (1..=4)
                .map(InstrumentId::new)
                .collect::<std::result::Result<_, _>>()?,
            5,
        )?
        .with_context(
            EVENT_REVISION,
            REGIME_REVISION,
            QUALITY_REVISION,
            Rules::new(before, after, age, allowed)?,
        )?)
    };
    let coverage = |known, interval, events| Coverage::Complete {
        source: [9; 32],
        known_at_ms: known,
        interval,
        events,
    };
    let contexts = || {
        data.iter()
            .map(|s| Input {
                instrument: s.selected.instrument,
                evidence: s.evidence,
                coverage: coverage(90, [0, 101], vec![]),
            })
            .collect::<Vec<_>>()
    };
    let constraints = || ConstraintInputs {
        volumes: &[],
        opens: &[],
    };
    let all = gate(
        1,
        0,
        10,
        vec![Regime::Falling, Regime::Flat, Regime::Rising],
    )?;
    let all_context = contexts();
    let out = governed::evaluate_with_context_inputs(
        request(),
        &evidence,
        &data,
        101,
        &all,
        intent,
        ContextInputs {
            constraints: constraints(),
            context: &all_context,
        },
    )?;
    assert_eq!(out.outcome().bytes(), render(&data)?.bytes());
    let flat = gate(1, 0, 10, vec![Regime::Flat])?;
    let narrowed = governed::evaluate_with_context_inputs(
        request(),
        &evidence,
        &data,
        101,
        &flat,
        intent,
        ContextInputs {
            constraints: constraints(),
            context: &all_context,
        },
    )?;
    assert_eq!(
        narrowed.decision().ok_or("decision")?.reasons(),
        [Reason::RegimeNotAllowed]
    );
    let ResearchRecord::Artifact(a) = narrowed.outcome().record() else {
        return Err("artifact".into());
    };
    assert_eq!(a.ranking.len(), 1);
    assert_eq!(a.ranking[0].instrument, format!("{:032x}", 3));
    assert_eq!(a.ranking[0].score_ppm, 0);
    assert_eq!(a.evidence.len(), 4);
    let mut tiny = inputs()?;
    tiny[2].closes[1].close = UsdPrice::new(100_000_001, 6)?;
    let exact = governed::evaluate_with_context_inputs(
        request(),
        &evidence,
        &tiny,
        101,
        &flat,
        intent,
        ContextInputs {
            constraints: constraints(),
            context: &all_context,
        },
    )?;
    assert_eq!(
        abstention(exact.outcome())?,
        AbstentionReason::NoEligibleCandidate
    );
    for point in [99, 100] {
        let mut event = contexts();
        event[2].coverage = coverage(90, [0, 101], vec![point]);
        let denied = governed::evaluate_with_context_inputs(
            request(),
            &evidence,
            &data,
            101,
            &flat,
            intent,
            ContextInputs {
                constraints: constraints(),
                context: &event,
            },
        )?;
        assert_eq!(
            abstention(denied.outcome())?,
            AbstentionReason::NoEligibleCandidate
        );
        assert_eq!(
            denied.decision().ok_or("decision")?.reasons(),
            [Reason::EventExcluded, Reason::RegimeNotAllowed]
        );
    }
    let mut extended = inputs()?;
    for s in &mut extended {
        s.rights_until_ms = 103;
    }
    let future_window = gate(0, 1, 10, vec![Regime::Flat])?;
    for (point, blocked) in [(101, true), (102, false)] {
        let future_context = extended
            .iter()
            .enumerate()
            .map(|(i, s)| Input {
                instrument: s.selected.instrument,
                evidence: s.evidence,
                coverage: coverage(90, [0, 103], if i == 2 { vec![point] } else { vec![] }),
            })
            .collect::<Vec<_>>();
        let out = governed::evaluate_with_context_inputs(
            request(),
            &evidence,
            &extended,
            101,
            &future_window,
            intent,
            ContextInputs {
                constraints: constraints(),
                context: &future_context,
            },
        )?;
        assert_eq!(
            matches!(out.outcome().record(), ResearchRecord::Abstention(_)),
            blocked
        );
    }
    let zero = gate(0, 0, 10, vec![Regime::Flat])?;
    let mut event = contexts();
    event[2].coverage = coverage(90, [0, 101], vec![99]);
    assert!(matches!(
        governed::evaluate_with_context_inputs(
            request(),
            &evidence,
            &data,
            101,
            &zero,
            intent,
            ContextInputs {
                constraints: constraints(),
                context: &event
            }
        )?
        .outcome()
        .record(),
        ResearchRecord::Artifact(_)
    ));
    for (changed, expected) in [
        (Coverage::Unknown, AbstentionReason::InsufficientHistory),
        (
            coverage(90, [0, 100], vec![]),
            AbstentionReason::InsufficientHistory,
        ),
        (
            coverage(101, [0, 101], vec![]),
            AbstentionReason::FutureEvidence,
        ),
        (
            coverage(89, [0, 101], vec![]),
            AbstentionReason::StaleEvidence,
        ),
        (
            coverage(90, [0, 102], vec![]),
            AbstentionReason::MissingEntitlement,
        ),
        (
            coverage(90, [0, 101], vec![99, 99]),
            AbstentionReason::InvalidEvidence,
        ),
    ] {
        let mut critical = contexts();
        critical[0].coverage = changed;
        let out = governed::evaluate_with_context_inputs(
            request(),
            &evidence,
            &data,
            101,
            &flat,
            intent,
            ContextInputs {
                constraints: constraints(),
                context: &critical,
            },
        )?;
        assert_eq!(abstention(out.outcome())?, expected);
        assert!(out.decision().is_none());
    }
    let stale = gate(0, 0, 9, vec![Regime::Flat])?;
    let mut fresh_calendar = contexts();
    for c in &mut fresh_calendar {
        c.coverage = coverage(95, [0, 101], vec![]);
    }
    assert_eq!(
        abstention(
            governed::evaluate_with_context_inputs(
                request(),
                &evidence,
                &data,
                101,
                &stale,
                intent,
                ContextInputs {
                    constraints: constraints(),
                    context: &fresh_calendar
                }
            )?
            .outcome()
        )?,
        AbstentionReason::StaleEvidence
    );
    assert_eq!(
        abstention(governed::evaluate(request(), &evidence, &data, 101, &all, intent)?.outcome())?,
        AbstentionReason::InsufficientHistory
    );
    let mut mixed = contexts();
    mixed[0].coverage = Coverage::Unknown;
    mixed[1].coverage = coverage(101, [0, 101], vec![]);
    for reverse in [false, true] {
        if reverse {
            mixed.reverse();
        }
        let out = governed::evaluate_with_context_inputs(
            request(),
            &evidence,
            &data,
            101,
            &all,
            intent,
            ContextInputs {
                constraints: constraints(),
                context: &mixed,
            },
        )?;
        assert_eq!(abstention(out.outcome())?, AbstentionReason::FutureEvidence);
    }
    let mut rebound = contexts();
    rebound[0].evidence = [0; 32];
    let invalid = governed::evaluate_with_context_inputs(
        request(),
        &evidence,
        &data,
        101,
        &all,
        intent,
        ContextInputs {
            constraints: constraints(),
            context: &rebound,
        },
    )?;
    assert_eq!(
        abstention(invalid.outcome())?,
        AbstentionReason::InvalidEvidence
    );
    assert!(
        Policy::new([8; 32], [3; 32], vec![InstrumentId::new(1)?], 1)?
            .with_context(
                "unknown",
                REGIME_REVISION,
                QUALITY_REVISION,
                Rules::new(0, 0, 10, vec![Regime::Flat])?
            )
            .is_err()
    );
    assert!(Rules::new(0, 0, 0, vec![Regime::Flat]).is_err());
    assert!(Rules::new(0, 0, 1, vec![Regime::Flat, Regime::Flat]).is_err());
    Ok(())
}

#[test]
fn independent_validation_reconciles_proposals_and_seals_typed_preparation() -> Result {
    use edgeagent_research_application::{
        governed::{self, ConstraintInputs, ContextInputs},
        publication::{EVALUATE, PreparedCompletion},
        validation::{self, Inputs},
    };
    use policy::{Direction, Intent, Policy};
    let data = inputs()?;
    let evidence = policy()?;
    let gate = Policy::new(
        [8; 32],
        [3; 32],
        (1..=4)
            .map(InstrumentId::new)
            .collect::<std::result::Result<_, _>>()?,
        5,
    )?;
    let intent = Intent {
        direction: Direction::Long,
        horizon_sessions: 1,
    };
    let selected = || Inputs {
        request: request(),
        evidence: &evidence,
        series: &data,
        expires_at_ms: 101,
        policy: &gate,
        intent,
        context: ContextInputs {
            constraints: ConstraintInputs {
                volumes: &[],
                opens: &[],
            },
            context: &[],
        },
    };
    let expected = render(&data)?;
    let validated = validation::validate(expected.bytes(), selected())?;
    assert_eq!(validated.bytes(), expected.bytes());
    assert_eq!(validated.digest(), expected.digest());
    assert!(validated.record() == expected.record());
    assert!(validated.decision().is_some());
    let command = |r: ResearchRequest| -> Result<edgeagent_contracts::MessageEnvelope> {
        let partition = format!("research-request/{}/{}", r.scope, r.request_id);
        let record = ResearchRecord::Request(r);
        Ok(edgeagent_contracts::MessageEnvelope::from_payload(
            edgeagent_contracts::MessageMetadata {
                id: "validator-delivery".into(),
                source: edgeagent_contracts::Component::Gateway.source_uri().into(),
                message_type: EVALUATE.message_type().into(),
                subject: partition.clone(),
                time: "2026-10-30T00:00:00Z".into(),
                data_schema: EVALUATE.data_schema().into(),
                correlation_id: "validation-correlation".into(),
                causation_id: "validation-cause".into(),
                idempotency_key: record.digest()?,
                partition_key: partition,
                trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
                trace_state: None,
            },
            &record,
        )?)
    };
    let prepared = PreparedCompletion::from_validated(
        command(request())?,
        &validated,
        "2026-10-30T00:00:00Z",
    )?;
    assert_eq!(prepared.outcome_bytes(), expected.bytes());
    let mut foreign = request();
    foreign.request_id = "foreign".into();
    assert!(
        PreparedCompletion::from_validated(command(foreign)?, &validated, "2026-10-30T00:00:00Z")
            .is_err()
    );
    for mutation in 0..10 {
        let ResearchRecord::Artifact(mut artifact) = expected.record().clone() else {
            return Err("artifact".into());
        };
        match mutation {
            0 => artifact.ranking[0].score_ppm += 1,
            1 => artifact.ranking[3].score_ppm -= 1,
            2 => {
                artifact.ranking.remove(0);
            }
            3 => artifact.evidence[0].digest = "09".repeat(32),
            4 => artifact.evidence[0].available_at_ms = 99,
            5 => artifact.expires_at_ms = 102,
            6 => artifact.explanation = "PRIVATE_SENTINEL".into(),
            7 => artifact.request.implementation = "08".repeat(32),
            8 => artifact.request.scope = "foreign".into(),
            _ => {
                artifact.ranking.pop();
                artifact.evidence.pop();
            }
        }
        // All mutations here remain canonical valid v1: shape checks alone cannot catch them.
        let bytes = ResearchRecord::Artifact(artifact).canonical_bytes()?;
        let error = validation::validate(&bytes, selected())
            .err()
            .ok_or("expected mismatch")?;
        assert!(matches!(error, validation::Error::Mismatch));
        assert!(!format!("{error:?}").contains("PRIVATE_SENTINEL"));
        assert_eq!(error.to_string(), "research proposal validation refused");
    }
    let fake =
        edgeagent_research_application::abstain(request(), AbstentionReason::NoEligibleCandidate)?;
    assert!(matches!(
        validation::validate(fake.bytes(), selected()),
        Err(validation::Error::Mismatch)
    ));
    assert!(matches!(
        validation::validate(
            &ResearchRecord::Request(request()).canonical_bytes()?,
            selected()
        ),
        Err(validation::Error::NotTerminal)
    ));
    let malformed = validation::validate(b"PRIVATE_SENTINEL", selected())
        .err()
        .ok_or("error")?;
    assert!(std::error::Error::source(&malformed).is_some());
    assert!(!format!("{malformed:?}").contains("PRIVATE_SENTINEL"));
    let oversize = vec![b'x'; edgeagent_contracts::research::MAX_RESEARCH_BYTES + 1];
    assert!(matches!(
        validation::validate(&oversize, selected()),
        Err(validation::Error::Proposal(_))
    ));
    let mut ties: serde_json::Value = serde_json::from_slice(expected.bytes())?;
    ties["data"]["ranking"]
        .as_array_mut()
        .ok_or("ranking")?
        .swap(0, 1);
    assert!(matches!(
        validation::validate(&serde_json::to_vec(&ties)?, selected()),
        Err(validation::Error::Proposal(_))
    ));
    let mut forged: serde_json::Value = serde_json::from_slice(expected.bytes())?;
    forged["data"]["validated"] = true.into();
    assert!(matches!(
        validation::validate(&serde_json::to_vec(&forged)?, selected()),
        Err(validation::Error::Proposal(_))
    ));
    let needs = Policy::new(
        [8; 32],
        [3; 32],
        (1..=4)
            .map(InstrumentId::new)
            .collect::<std::result::Result<_, _>>()?,
        5,
    )?
    .with_liquidity(liquidity::REVISION, 0)?;
    let mut missing = selected();
    missing.policy = &needs;
    assert!(matches!(
        validation::validate(expected.bytes(), missing),
        Err(validation::Error::Mismatch)
    ));

    let mut invalid = selected();
    invalid.expires_at_ms = 102;
    let failure = validation::validate(expected.bytes(), invalid)
        .err()
        .ok_or("evaluation failure")?;
    assert!(matches!(failure, validation::Error::Evaluation(_)));
    assert!(std::error::Error::source(&failure).is_some());
    let narrower = Policy::new(
        [8; 32],
        [3; 32],
        vec![InstrumentId::new(1)?, InstrumentId::new(3)?],
        5,
    )?;
    let proposal = governed::evaluate(request(), &evidence, &data, 101, &narrower, intent)?;
    let mut narrow_inputs = selected();
    narrow_inputs.policy = &narrower;
    assert!(validation::validate(proposal.outcome().bytes(), narrow_inputs).is_ok());
    let mut failed = inputs()?;
    failed[0].entitled = false;
    let abstained =
        edgeagent_research_application::abstain(request(), AbstentionReason::MissingEntitlement)?;
    let mut denied = selected();
    denied.series = &failed;
    let receipt = validation::validate(abstained.bytes(), denied)?;
    assert!(receipt.decision().is_none());
    let mut denied = selected();
    denied.series = &failed;
    assert!(matches!(
        validation::validate(expected.bytes(), denied),
        Err(validation::Error::Mismatch)
    ));
    Ok(())
}
