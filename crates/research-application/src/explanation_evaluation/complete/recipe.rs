//! Invented bounded owner assertions; never an authorization adapter or live market data.
use super::*;
use crate::{
    execution::Selection,
    governed::{ConstraintInputs, ContextInputs},
    worker::{ExecutionEvidence, LoadedEvidence},
};
use edgeagent_contracts::{
    research::ResearchRequest,
    strategy::{Configuration, ExecutionConfiguration, Regime},
};
use edgeagent_market_data_domain::{InstrumentId, ListingId, SessionDate, ShareVolume, UsdPrice};
use edgeagent_research_domain::{
    AdjustmentBasis, EvidencePolicy, FrozenClose, FrozenSeries, PolicyBinding, SelectedInstrument,
    context_policy as context, liquidity, market_constraints,
};
use serde_json::json;
pub(super) struct Fixture {
    pub record: ResearchRecord,
    pub decision: DecisionProvenance,
    pub configuration: Vec<u8>,
    pub input: ExecutionEvidence,
    pub assertions: serde_json::Value,
}
pub(super) fn build(n: usize) -> Result<Fixture, EvaluationError> {
    let count = match n {
        1 | 9 | 13..=15 | 17 | 20 | 22 | 23 | 25 | 27..=30 => 1,
        3 | 6 | 26 => 3,
        _ => 2,
    };
    let mut prices = vec![[1_000_000_i128, 1_005_000, 1_010_000]; count];
    if count > 1 {
        prices[1] = [1_000_000, 1_002_500, 1_005_000];
    }
    if count > 2 {
        prices[2] = [1_000_000, 1_001_000, 1_002_000];
    }
    let mut volumes = vec![100; count];
    let mut events = vec![Vec::<i64>::new(); count];
    let mut config = ExecutionConfiguration::decode(include_bytes!(
        "../../../../contracts/fixtures/strategy/execution-v1.json"
    ))
    .map_err(|_| INVALID)?;
    let ExecutionConfiguration::V1 {
        bundle,
        context_max_age_ms,
        allowed_regimes,
        ..
    } = &mut config;
    *context_max_age_ms = 10;
    let Configuration::V1 {
        strategy, policy, ..
    } = bundle;
    strategy.lookback_sessions = if n == 28 { 1 } else { 2 };
    strategy.expiry_ms = if n == 27 { 1 } else { 10 };
    policy.instruments = (1..=count).map(|id| format!("{id:032x}")).collect();
    match n {
        3 => {
            prices[0] = [1_000_000, 1_010_000, 1_020_001];
            prices[1] = [1_000_000, 1_010_000, 1_020_000];
            prices[2] = [1_000_000, 1_002_500, 1_005_000];
        }
        4 => prices[1] = prices[0],
        5 => {
            prices[0] = [1_000_000, 995_000, 990_000];
            prices[1] = [1_000_000, 990_000, 980_000];
        }
        6 => {
            prices[1] = [1_000_000; 3];
            prices[2] = [1_000_000, 995_000, 990_000];
        }
        8 => {
            volumes[1] = 0;
            policy.min_daily_turnover_cents = 1;
        }
        9 => {
            prices[0] = [1_000_000; 3];
            policy.min_daily_turnover_cents = 1_000_000;
        }
        10 | 12 => {
            prices.fill([1_000_000; 3]);
            prices[1] = [1_000_000, 1_100_000, 1_000_000];
            policy.max_volatility_ppm = 0;
        }
        11 => prices.fill([1_000_000; 3]),
        14 => strategy.horizon_sessions = policy.max_horizon_sessions,
        16 => {
            prices[0] = [1_000_000; 3];
            prices[1] = [1_000_000, 995_000, 990_000];
            *allowed_regimes = vec![Regime::Flat, Regime::Rising];
        }
        17 => {
            prices[0] = [1_000_000; 3];
            *allowed_regimes = vec![Regime::Flat];
        }
        18 | 19 => {
            prices.fill([1_000_000; 3]);
            policy.event_before_ms = 5;
            policy.event_after_ms = 5;
            events[1] = vec![if n == 18 { 95 } else { 105 }];
        }
        26 => {
            prices.fill([1_000_000; 3]);
            volumes[1] = 0;
            policy.min_daily_turnover_cents = 1;
        }
        28 => strategy.horizon_sessions = 3,
        _ => {}
    }
    let mut opens: Vec<_> = prices.iter().map(|p| [p[0], p[0], p[1]]).collect();
    if matches!(n, 11 | 12 | 26) {
        policy.max_gap_ppm = 0;
        opens[if n == 26 { 2 } else { 1 }][1] = 1_100_000;
    }
    let lookback = strategy.lookback_sessions;
    let expiry = strategy.expiry_ms;
    let bytes = config.canonical_bytes().map_err(|_| INVALID)?;
    let selection = Selection::decode(&bytes).map_err(|_| INVALID)?;
    let profile = selection.profile("evaluation");
    let mut configuration = [0; 32];
    for (i, b) in configuration.iter_mut().enumerate() {
        *b = u8::from_str_radix(&profile.configuration[i * 2..i * 2 + 2], 16)
            .map_err(|_| INVALID)?;
    }
    let dates = (1..=u32::from(lookback) + 1)
        .map(|d| SessionDate::new(2026, 10, d))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| INVALID)?;
    let universe = (1..=count)
        .map(|id| {
            Ok(SelectedInstrument {
                instrument: InstrumentId::new(id as u128).map_err(|_| INVALID)?,
                listing: ListingId::new(id as u128).map_err(|_| INVALID)?,
                mapping: [1; 32],
            })
        })
        .collect::<Result<Vec<_>, EvaluationError>>()?;
    let evidence = EvidencePolicy::new(
        PolicyBinding {
            decision_at_ms: 100,
            max_age_ms: 10,
            implementation: [2; 32],
            configuration,
            calendar: [4; 32],
        },
        universe.clone(),
        dates.clone(),
        lookback,
    )
    .map_err(|_| INVALID)?;
    let series = universe
        .into_iter()
        .enumerate()
        .map(|(i, selected)| {
            Ok(FrozenSeries {
                selected,
                evidence: [5 + i as u8; 32],
                implementation: [2; 32],
                configuration,
                calendar: [4; 32],
                known_at_ms: if n == 21 && i == 0 { 91 } else { 90 },
                rights_until_ms: 110,
                entitled: true,
                quality_accepted: true,
                basis: AdjustmentBasis::Raw,
                closes: dates
                    .iter()
                    .enumerate()
                    .map(|(j, date)| {
                        Ok(FrozenClose {
                            session: *date,
                            close: UsdPrice::new(prices[i][j], 4).map_err(|_| INVALID)?,
                            event_at_ms: 90 - (i64::from(lookback) - j as i64) * 10
                                + i64::from(n == 21 && i == 0 && j == usize::from(lookback)),
                            observed_at_ms: 95,
                            available_at_ms: 100,
                        })
                    })
                    .collect::<Result<Vec<_>, EvaluationError>>()?,
            })
        })
        .collect::<Result<Vec<_>, EvaluationError>>()?;
    let volume_inputs = series
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Ok(liquidity::Input {
                instrument: s.selected.instrument,
                evidence: s.evidence,
                rows: dates
                    .iter()
                    .map(|d| {
                        Ok(liquidity::Volume {
                            session: *d,
                            shares: ShareVolume::new(volumes[i], 0).map_err(|_| INVALID)?,
                        })
                    })
                    .collect::<Result<Vec<_>, EvaluationError>>()?,
            })
        })
        .collect::<Result<Vec<_>, EvaluationError>>()?;
    let open_inputs = series
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Ok(market_constraints::Input {
                instrument: s.selected.instrument,
                evidence: s.evidence,
                rows: dates
                    .iter()
                    .enumerate()
                    .map(|(j, d)| {
                        Ok(market_constraints::Open {
                            session: *d,
                            price: UsdPrice::new(opens[i][j], 4).map_err(|_| INVALID)?,
                            action: false,
                        })
                    })
                    .collect::<Result<Vec<_>, EvaluationError>>()?,
            })
        })
        .collect::<Result<Vec<_>, EvaluationError>>()?;
    let contexts = series
        .iter()
        .enumerate()
        .map(|(i, s)| context::Input {
            instrument: s.selected.instrument,
            evidence: s.evidence,
            coverage: context::Coverage::Complete {
                source: [9; 32],
                known_at_ms: if n == 21 && i == 0 { 91 } else { 90 },
                interval: [0, 110],
                events: events[i].clone(),
            },
        })
        .collect();
    let input = ExecutionEvidence {
        loaded: LoadedEvidence {
            policy: evidence,
            series,
            expires_at_ms: 100 + i64::from(expiry),
        },
        volumes: volume_inputs,
        opens: open_inputs,
        context: contexts,
    };
    let request = ResearchRequest {
        request_id: format!("complete-U{n:02}"),
        scope: "evaluation".into(),
        implementation: profile.implementation,
        configuration: profile.configuration,
        decision_at_ms: 100,
        lookback_sessions: lookback,
        instruments: (1..=count).map(|id| format!("{id:032x}")).collect(),
    };
    let execution = selection
        .validate(
            request,
            &input.loaded.policy,
            &input.loaded.series,
            input.loaded.expires_at_ms,
            ContextInputs {
                constraints: ConstraintInputs {
                    volumes: &input.volumes,
                    opens: &input.opens,
                },
                context: &input.context,
            },
        )
        .map_err(|_| INVALID)?;
    let selected_prices: Vec<_> = prices.iter().map(|rows| &rows[..dates.len()]).collect();
    let selected_opens: Vec<_> = opens.iter().map(|rows| &rows[..dates.len()]).collect();
    let assertions = json!({"configuration":serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_|INVALID)?,"events":events,"opens":selected_opens,"prices":selected_prices,"template":"invented-selected-daily-v1; mapping=01;calendar=04;daily=05+index;event-source=09;known=90(+1-U21-first);latest-event=90(+1-U21-first);observed=95;available=100;rights=110;raw-usd-scale=4;shares-scale=0;event-coverage=[0,110)","volumes":volumes});
    Ok(Fixture {
        record: execution.record().clone(),
        decision: execution.provenance().map_err(|_| INVALID)?,
        configuration: bytes,
        input,
        assertions,
    })
}
