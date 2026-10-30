use edgeagent_market_data_domain::{InstrumentId, ListingId, SessionDate, ShareVolume, UsdPrice};
use edgeagent_research_domain::{
    liquidity::{self, Input, Liquidity, Volume},
    *,
};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn measure(
    values: &[(i128, u32, i128, u32)],
    change: impl FnOnce(&mut Input),
) -> Result<Liquidity> {
    let selected = SelectedInstrument {
        instrument: InstrumentId::new(1)?,
        listing: ListingId::new(1)?,
        mapping: [1; 32],
    };
    let sessions = (1..=values.len())
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
        u16::try_from(values.len() - 1)?,
    )?;
    let input = FrozenSeries {
        selected,
        evidence: [5; 32],
        implementation: [2; 32],
        configuration: [3; 32],
        calendar: [4; 32],
        known_at_ms: 90,
        rights_until_ms: 101,
        entitled: true,
        quality_accepted: true,
        basis: AdjustmentBasis::Raw,
        closes: values
            .iter()
            .zip(&sessions)
            .enumerate()
            .map(|(i, (v, s))| {
                Ok(FrozenClose {
                    session: *s,
                    close: UsdPrice::new(v.0, v.1)?,
                    event_at_ms: 90 + i as i64,
                    observed_at_ms: 99,
                    available_at_ms: 100,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    };
    let mut volume = Input {
        instrument: selected.instrument,
        evidence: [5; 32],
        rows: values
            .iter()
            .zip(sessions)
            .map(|(v, s)| {
                Ok(Volume {
                    session: s,
                    shares: ShareVolume::new(v.2, v.3)?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    };
    change(&mut volume);
    let series = [input];
    let eligible = policy.assess(&series).map_err(|_| "evidence")?;
    Ok(liquidity::calculate(
        eligible.series().first().ok_or("series")?,
        &volume,
    )?)
}
#[test]
fn exact_vectors_floor_once_and_keep_zero_and_large_values() -> Result {
    assert_eq!(measure(&[(19, 3, 1, 0), (1, 3, 1, 0)], |_| {})?.cents(), 1);
    assert_eq!(
        measure(&[(123, 2, 15, 1), (123, 2, 15, 1)], |_| {})?.cents(),
        184
    );
    assert_eq!(
        measure(&[(123, 2, 0, 0), (123, 2, 0, 0)], |_| {})?.cents(),
        0
    );
    assert_eq!(
        measure(&[(10_i128.pow(28) - 1, 8, 1, 6); 2], |_| {})?.cents(),
        9_999_999_999_999_999
    );
    assert_eq!(
        measure(&[(10_i128.pow(28) - 1, 0, 0, 0); 2], |_| {})?.cents(),
        0
    );
    assert!(
        measure(
            &[(10_i128.pow(28) - 1, 0, 10_i128.pow(28) - 1, 0); 2],
            |_| {}
        )
        .is_err()
    );
    assert!(measure(&[(10_i128.pow(20), 0, 1, 0); 2], |_| {}).is_err());
    assert_eq!(
        liquidity::REVISION,
        "mean-selected-close-volume-floor-cents-v1"
    );
    Ok(())
}
#[test]
fn exact_small_rational_oracle_and_row_order_agree() -> Result {
    for p in 1..=10 {
        for q in 1..=10 {
            for v in 0..=3 {
                assert_eq!(
                    measure(&[(p, 2, v, 1), (q, 2, 3 - v, 1)], |x| x.rows.reverse())?.cents(),
                    u64::try_from((p * v + q * (3 - v)) / 20)?
                );
            }
        }
    }
    Ok(())
}
#[test]
fn missing_duplicate_foreign_or_rebound_volume_rejects() -> Result {
    let values = [(100, 0, 1, 0); 2];
    let missing = measure(&values, |x| {
        x.rows.pop();
    })
    .err()
    .ok_or("missing error")?;
    assert_eq!(
        missing.downcast_ref::<liquidity::Error>(),
        Some(&liquidity::Error::InsufficientHistory)
    );
    assert!(measure(&values, |x| x.rows[1].session = x.rows[0].session).is_err());
    assert!(measure(&values, |x| x.evidence = [9; 32]).is_err());
    let foreign = SessionDate::new(2026, 10, 3)?;
    assert!(measure(&values, |x| x.rows[1].session = foreign).is_err());
    assert_eq!(
        liquidity::Error::OutOfRange.to_string(),
        "liquidity calculation refused"
    );
    Ok(())
}
