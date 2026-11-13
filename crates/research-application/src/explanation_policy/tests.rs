use super::*;
use crate::{execution::Selection, worker::ExecutionEvidence};
use crate::{
    governed::{ConstraintInputs, ContextInputs},
    worker::LoadedEvidence,
};
use edgeagent_contracts::explanation::Variant;
use edgeagent_contracts::{
    research::ResearchRecord,
    research_decision::DecisionProvenance,
    strategy::{Configuration, ExecutionConfiguration, Regime},
};
use edgeagent_market_data_domain::{InstrumentId, ListingId, SessionDate, ShareVolume, UsdPrice};
use edgeagent_research_domain::{
    AdjustmentBasis, EvidencePolicy, FrozenClose, FrozenSeries, PolicyBinding, SelectedInstrument,
};
use edgeagent_research_domain::{context_policy as context, liquidity, market_constraints};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn fixture(
    event: i64,
) -> Result<(
    ResearchRecord,
    DecisionProvenance,
    Vec<u8>,
    ExecutionEvidence,
)> {
    let mut config = ExecutionConfiguration::decode(include_bytes!(
        "../../../contracts/fixtures/strategy/execution-v1.json"
    ))?;
    let ExecutionConfiguration::V1 {
        bundle,
        allowed_regimes,
        ..
    } = &mut config;
    *allowed_regimes = vec![Regime::Flat];
    let Configuration::V1 {
        strategy, policy, ..
    } = bundle;
    strategy.lookback_sessions = 2;
    strategy.expiry_ms = 10;
    policy.instruments = (1..=3).map(|id| format!("{id:032x}")).collect();
    policy.min_daily_turnover_cents = 1_000_000;
    policy.max_volatility_ppm = 0;
    policy.max_gap_ppm = 0;
    policy.event_before_ms = 5;
    policy.event_after_ms = 5;
    let bytes = config.canonical_bytes()?;
    let selected = Selection::decode(&bytes)?;
    let configuration: [u8; 32] = (0..32)
        .map(|i| {
            u8::from_str_radix(
                &selected.profile("offline").configuration[i * 2..i * 2 + 2],
                16,
            )
        })
        .collect::<std::result::Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| "digest")?;
    let sessions = (1..=3)
        .map(|d| SessionDate::new(2026, 10, d))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let universe = (1..=3)
        .map(|id| {
            Ok(SelectedInstrument {
                instrument: InstrumentId::new(id)?,
                listing: ListingId::new(id)?,
                mapping: [1; 32],
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let policy = EvidencePolicy::new(
        PolicyBinding {
            decision_at_ms: 100,
            max_age_ms: 10,
            implementation: [2; 32],
            configuration,
            calendar: [4; 32],
        },
        universe.clone(),
        sessions.clone(),
        2,
    )?;
    let series = universe
        .into_iter()
        .enumerate()
        .map(|(i, selected)| {
            Ok(FrozenSeries {
                selected,
                evidence: [i as u8 + 5; 32],
                implementation: [2; 32],
                configuration,
                calendar: [4; 32],
                known_at_ms: 90,
                rights_until_ms: 110,
                entitled: true,
                quality_accepted: true,
                basis: AdjustmentBasis::Raw,
                closes: sessions
                    .iter()
                    .enumerate()
                    .map(|(j, session)| {
                        Ok(FrozenClose {
                            session: *session,
                            close: UsdPrice::new(
                                if i == 1 && j == 1 {
                                    110
                                } else if i == 2 && j == 2 {
                                    90
                                } else {
                                    100
                                },
                                0,
                            )?,
                            event_at_ms: 70 + j as i64 * 10,
                            observed_at_ms: 85 + j as i64 * 5,
                            available_at_ms: 90 + j as i64 * 5,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let volumes = series
        .iter()
        .map(|s| {
            Ok(liquidity::Input {
                instrument: s.selected.instrument,
                evidence: s.evidence,
                rows: s
                    .closes
                    .iter()
                    .map(|c| {
                        Ok(liquidity::Volume {
                            session: c.session,
                            shares: ShareVolume::new(100, 0)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let opens = series
        .iter()
        .map(|s| market_constraints::Input {
            instrument: s.selected.instrument,
            evidence: s.evidence,
            rows: s
                .closes
                .iter()
                .map(|c| market_constraints::Open {
                    session: c.session,
                    price: c.close,
                    action: false,
                })
                .collect(),
        })
        .collect();
    let context = series
        .iter()
        .enumerate()
        .map(|(i, s)| context::Input {
            instrument: s.selected.instrument,
            evidence: s.evidence,
            coverage: context::Coverage::Complete {
                source: [9; 32],
                known_at_ms: 90,
                interval: [0, 110],
                events: if i == 1 { vec![event] } else { vec![] },
            },
        })
        .collect();
    let input = ExecutionEvidence {
        loaded: LoadedEvidence {
            policy,
            series,
            expires_at_ms: 110,
        },
        volumes,
        opens,
        context,
    };
    let request = edgeagent_contracts::research::ResearchRequest {
        request_id: "policy-preview".into(),
        scope: "offline".into(),
        implementation: "02".repeat(32),
        configuration: selected.profile("offline").configuration,
        decision_at_ms: 100,
        lookback_sessions: 2,
        instruments: (1..=3).map(|id| format!("{id:032x}")).collect(),
    };
    let validated = selected.validate(
        request,
        &input.loaded.policy,
        &input.loaded.series,
        110,
        ContextInputs {
            constraints: ConstraintInputs {
                volumes: &input.volumes,
                opens: &input.opens,
            },
            context: &input.context,
        },
    )?;
    Ok((
        validated.record().clone(),
        validated.provenance()?,
        bytes,
        input,
    ))
}
#[test]
fn measurements_boundaries_and_adverse_sources_survive_variants_and_replay() -> Result {
    for event in [95, 105] {
        let (record, decision, config, mut input) = fixture(event)?;
        let preview = PolicyPreview::from_verified(&record, &decision, &config, &input)?;
        let mut proposal = crate::explanation::Allowlist::from_verified(&record, &decision)?
            .standard()?
            .proposal()
            .clone();
        for variant in [Variant::Standard, Variant::PlainLanguage] {
            proposal.claims[0].variant = variant;
            let bytes = proposal.canonical_bytes()?;
            let text = preview.validate(&bytes)?;
            for required in [
                "research-explanation-policy.v1",
                "turnover_cents=1000000 minimum_inclusive=1000000 below_minimum=false",
                "volatility_ppm=95455 maximum_inclusive=0 exceeded=true",
                "volatility_ppm=0 maximum_inclusive=0 exceeded=false",
                "gap_ppm=0 maximum_inclusive=0 exceeded=false",
                "gap_ppm=100000 maximum_inclusive=0 exceeded=true",
                "regime=falling allowed=flat denied=true",
                "window_start_ms=95 window_end_ms=105 endpoints=inclusive",
                "coverage=complete",
                "conflict=true",
                "market_age_ms=10 context_age_ms=10 maximum_inclusive_ms=10",
                "event_at_ms=90 observed_at_ms=95 available_at_ms=100",
                "Scenarios and risk illustrations: not qualified.",
            ] {
                assert!(text.contains(required), "missing {required}");
            }
            assert!(text.lines().any(
                |line| line.contains("volatility_ppm=95455") && line.contains(&"06".repeat(32))
            ));
            assert!(
                text.lines()
                    .any(|line| line.contains("gap_ppm=100000") && line.contains(&"06".repeat(32)))
            );
            assert!(text.lines().any(|line| line.contains("conflict=true")
                && line.contains(&"06".repeat(32))
                && line.contains(&"09".repeat(32))));
            assert!(!text.contains("event_at_ms=unavailable"));
            assert!(text.contains("events_count=0 conflict=false"));
            assert!(text.contains(&format!(
                "calendar_event_at_ms={event} within_closed_window=true"
            )));
            assert_eq!(
                text,
                PolicyPreview::from_verified(&record, &decision, &config, &input)?
                    .validate(&bytes)?
            );
        }
        input.loaded.series.reverse();
        input.volumes.reverse();
        input.opens.reverse();
        input.context.reverse();
        assert_eq!(
            preview.standard()?,
            PolicyPreview::from_verified(&record, &decision, &config, &input)?.standard()?
        );
    }
    Ok(())
}
#[test]
fn changed_bindings_unknown_coverage_and_missing_measurements_refuse() -> Result {
    let (record, decision, config, mut input) = fixture(95)?;
    let mut changed = record.clone();
    if let ResearchRecord::Artifact(a) = &mut changed {
        a.ranking[0].score_ppm += 1;
    }
    let mut rebound = decision.clone();
    rebound.outcome = changed.digest()?;
    assert!(PolicyPreview::from_verified(&changed, &rebound, &config, &input).is_err());
    let mut false_reason = decision.clone();
    if let edgeagent_contracts::research_decision::Assessment::Evaluated { reasons, .. } =
        &mut false_reason.assessment
    {
        reasons.insert(
            0,
            edgeagent_contracts::research_decision::Reason::DirectionNotAllowed,
        );
    }
    assert!(PolicyPreview::from_verified(&record, &false_reason, &config, &input).is_err());
    let mut other_config = ExecutionConfiguration::decode(&config)?;
    let ExecutionConfiguration::V1 {
        context_max_age_ms, ..
    } = &mut other_config;
    *context_max_age_ms += 1;
    assert!(
        PolicyPreview::from_verified(&record, &decision, &other_config.canonical_bytes()?, &input)
            .is_err()
    );
    let mut foreign = decision.clone();
    foreign.policy = "aa".repeat(32);
    assert!(PolicyPreview::from_verified(&record, &foreign, &config, &input).is_err());
    input.volumes[0].evidence = [99; 32];
    assert!(PolicyPreview::from_verified(&record, &decision, &config, &input).is_err());
    input.volumes[0].evidence = [5; 32];
    input.opens[0].rows.pop();
    assert!(PolicyPreview::from_verified(&record, &decision, &config, &input).is_err());
    let (record, decision, config, mut input) = fixture(95)?;
    input.context[0].coverage = context::Coverage::Unknown;
    assert!(PolicyPreview::from_verified(&record, &decision, &config, &input).is_err());
    Ok(())
}
