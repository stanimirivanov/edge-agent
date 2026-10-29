use edgeagent_market_data_domain::{InstrumentId, ListingId, SessionDate, UsdPrice};
use edgeagent_research_domain::{momentum::*, *};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn calculate_prices(prices: &[UsdPrice], reverse: bool) -> Result<Momentum> {
    let selected = SelectedInstrument {
        instrument: InstrumentId::new(1)?,
        listing: ListingId::new(1)?,
        mapping: [1; 32],
    };
    let sessions = (1..=prices.len())
        .map(|d| SessionDate::new(2026, 10, d as u32))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let policy = EvidencePolicy::new(
        PolicyBinding {
            decision_at_ms: 100,
            max_age_ms: 100,
            implementation: [2; 32],
            configuration: [3; 32],
            calendar: [4; 32],
        },
        vec![selected],
        sessions.clone(),
        u16::try_from(prices.len() - 1)?,
    )?;
    let mut closes = prices
        .iter()
        .zip(sessions)
        .enumerate()
        .map(|(i, (close, session))| FrozenClose {
            session,
            close: *close,
            event_at_ms: i as i64,
            observed_at_ms: 10,
            available_at_ms: 100,
        })
        .collect::<Vec<_>>();
    if reverse {
        closes.reverse();
    }
    let inputs = [FrozenSeries {
        selected,
        evidence: [5; 32],
        implementation: [2; 32],
        configuration: [3; 32],
        calendar: [4; 32],
        known_at_ms: 10,
        rights_until_ms: 101,
        entitled: true,
        quality_accepted: true,
        basis: AdjustmentBasis::Raw,
        closes,
    }];
    let eligible = policy
        .assess(&inputs)
        .map_err(|_| "unexpected evidence abstention")?;
    Ok(calculate(
        eligible.series().first().ok_or("missing candidate")?,
    )?)
}
fn ppm(first: i128, last: i128) -> Result<i64> {
    Ok(calculate_prices(&[UsdPrice::new(first, 0)?, UsdPrice::new(last, 0)?], false)?.score_ppm())
}
#[test]
fn exact_reference_vectors_include_positive_zero_and_negative_returns() -> Result {
    for (first, last, expected) in [
        (100, 110, 100_000),
        (100, 100, 0),
        (100, 90, -100_000),
        (3, 4, 333_333),
        (3, 2, -333_333),
        (1, 2, 1_000_000),
        (200, 1, -995_000),
    ] {
        assert_eq!(ppm(first, last)?, expected);
    }
    Ok(())
}
#[test]
fn half_ppm_ties_round_to_even_symmetrically() -> Result {
    for (last, expected) in [
        (2_000_001, 0),
        (2_000_003, 2),
        (1_999_999, 0),
        (1_999_997, -2),
        (2_000_005, 2),
        (1_999_995, -2),
    ] {
        assert_eq!(ppm(2_000_000, last)?, expected);
    }
    assert_eq!(ppm(4_000_000, 4_000_001)?, 0);
    assert_eq!(ppm(4_000_000, 4_000_003)?, 1);
    Ok(())
}
#[test]
fn equivalent_scales_and_large_valid_intermediates_are_exact() -> Result {
    let a = calculate_prices(&[UsdPrice::new(100, 0)?, UsdPrice::new(110, 0)?], false)?;
    let b = calculate_prices(
        &[UsdPrice::new(10000, 2)?, UsdPrice::new(110000, 3)?],
        false,
    )?;
    assert!(a == b);
    assert_eq!(ppm(8 * 10_i128.pow(27), 9 * 10_i128.pow(27))?, 125_000);
    assert_eq!(ppm(10_i128.pow(28) - 1, 10_i128.pow(28) - 1)?, 0);
    let small = calculate_prices(&[UsdPrice::new(1, 8)?, UsdPrice::new(2, 8)?], false)?;
    assert_eq!(small.score_ppm(), 1_000_000);
    Ok(())
}
#[test]
fn score_range_is_checked_without_clamping_or_payload_diagnostics() -> Result {
    assert_eq!(ppm(1_000_000, i128::from(i64::MAX) + 1_000_000)?, i64::MAX);
    let error = calculate_prices(
        &[
            UsdPrice::new(1_000_000, 0)?,
            UsdPrice::new(i128::from(i64::MAX) + 1_000_001, 0)?,
        ],
        false,
    )
    .err()
    .ok_or("expected range error")?;
    assert_eq!(error.to_string(), "momentum score out of range");
    assert!(
        calculate_prices(
            &[UsdPrice::new(1, 8)?, UsdPrice::new(10_i128.pow(28) - 1, 0)?],
            false
        )
        .is_err()
    );
    assert!(UsdPrice::new(0, 0).is_err());
    assert!(UsdPrice::new(-1, 0).is_err());
    Ok(())
}
#[test]
fn complete_session_lookback_and_revision_are_retained() -> Result {
    let prices = [
        UsdPrice::new(100, 0)?,
        UsdPrice::new(500, 0)?,
        UsdPrice::new(110, 0)?,
    ];
    let a = calculate_prices(&prices, false)?;
    let b = calculate_prices(&prices, true)?;
    assert!(a == b);
    assert_eq!(a.score_ppm(), 100_000);
    assert_eq!(a.lookback_sessions(), 2);
    assert_eq!(
        a.endpoint_sessions(),
        (
            SessionDate::new(2026, 10, 1)?,
            SessionDate::new(2026, 10, 3)?
        )
    );
    assert_eq!(a.revision(), "raw-usd-roc-ppm-half-even-v1");
    assert!(a.basis() == AdjustmentBasis::Raw);
    Ok(())
}
#[test]
fn exact_rational_error_is_bounded_by_half_a_ppm() -> Result {
    assert_eq!(MAX_ERROR_HALF_PPM, 1);
    for first in 1..=97 {
        for last in 1..=193 {
            let actual = i128::from(ppm(first, last)?);
            // Exact integer oracle: |rounded*d - numerator| <= d/2.
            let residual = (actual * first - (last - first) * PPM_PER_RETURN).abs();
            assert!(2 * residual <= first);
        }
    }
    Ok(())
}
