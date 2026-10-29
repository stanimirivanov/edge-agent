use edgeagent_market_data_domain::{InstrumentId, ListingId, SessionDate, UsdPrice};
use edgeagent_research_domain::*;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn selected(id: u128) -> Result<SelectedInstrument> {
    Ok(SelectedInstrument {
        instrument: InstrumentId::new(id)?,
        listing: ListingId::new(id)?,
        mapping: [1; 32],
    })
}
fn sessions() -> Result<Vec<SessionDate>> {
    Ok(vec![
        SessionDate::new(2026, 10, 26)?,
        SessionDate::new(2026, 10, 27)?,
    ])
}
fn binding() -> PolicyBinding {
    PolicyBinding {
        decision_at_ms: 100,
        max_age_ms: 10,
        implementation: [2; 32],
        configuration: [3; 32],
        calendar: [4; 32],
    }
}
fn policy() -> Result<EvidencePolicy> {
    Ok(EvidencePolicy::new(
        binding(),
        vec![selected(1)?, selected(2)?],
        sessions()?,
        1,
    )?)
}
fn series(id: u128) -> Result<FrozenSeries> {
    Ok(FrozenSeries {
        selected: selected(id)?,
        evidence: [5; 32],
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
                    close: UsdPrice::new(100 + i as i128, 2)?,
                    event_at_ms: 80 + i as i64 * 10,
                    observed_at_ms: 90,
                    available_at_ms: 100,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    })
}
fn reason(inputs: &[FrozenSeries]) -> Result<Abstention> {
    policy()?
        .assess(inputs)
        .err()
        .ok_or_else(|| "expected abstention".into())
}
#[test]
fn complete_evidence_is_sealed_in_historical_order() -> Result {
    let mut inputs = vec![series(2)?, series(1)?];
    inputs[0].closes.reverse();
    let policy = policy()?;
    let eligible = policy
        .assess(&inputs)
        .map_err(|_| "unexpected abstention")?;
    assert!(eligible.series()[0].frozen().selected.instrument == InstrumentId::new(1)?);
    assert!(eligible.series()[1].closes()[0].session < eligible.series()[1].closes()[1].session);
    assert_eq!(eligible.series()[0].frozen().evidence, [5; 32]);
    // Boundary inclusive: available at cutoff, close age exactly max_age.
    assert_eq!(eligible.series()[0].closes()[1].available_at_ms, 100);
    Ok(())
}
#[test]
fn time_rights_and_history_boundaries_fail_closed() -> Result {
    let good = vec![series(1)?, series(2)?];
    let mut input = good.clone();
    input[0].closes[1].available_at_ms = 101;
    assert_eq!(reason(&input)?, Abstention::FutureEvidence);
    let mut input = good.clone();
    input[0].known_at_ms = 101;
    for close in &mut input[0].closes {
        close.available_at_ms = 101;
    }
    assert_eq!(reason(&input)?, Abstention::FutureEvidence);
    let mut input = good.clone();
    input[0].closes[1].event_at_ms = 89;
    assert_eq!(reason(&input)?, Abstention::StaleEvidence);
    let mut input = good.clone();
    input[0].rights_until_ms = 100;
    assert_eq!(reason(&input)?, Abstention::MissingEntitlement);
    let mut input = good.clone();
    input[0].closes.remove(0);
    assert_eq!(reason(&input)?, Abstention::InsufficientHistory);
    assert_eq!(reason(&good[..1])?, Abstention::InsufficientHistory);
    let mut input = good;
    input[0].closes[1].observed_at_ms = 101;
    assert_eq!(reason(&input)?, Abstention::InvalidEvidence);
    Ok(())
}
#[test]
fn revisions_units_lineage_and_duplicate_sessions_cannot_pass() -> Result {
    let base = vec![series(1)?, series(2)?];
    for field in 0..8 {
        let mut input = base.clone();
        match field {
            0 => input[0].implementation = [9; 32],
            1 => input[0].configuration = [9; 32],
            2 => input[0].calendar = [9; 32],
            3 => input[0].selected.mapping = [9; 32],
            4 => input[0].evidence = [0; 32],
            5 => input[0].basis = AdjustmentBasis::SplitAdjusted,
            6 => input[0].quality_accepted = false,
            _ => input[0].selected.listing = ListingId::new(9)?,
        };
        assert_eq!(reason(&input)?, Abstention::InvalidEvidence);
    }
    let mut input = base.clone();
    input[0].closes[1] = input[0].closes[0].clone();
    assert_eq!(reason(&input)?, Abstention::InvalidEvidence);
    let mut input = base;
    input.push(series(1)?);
    assert_eq!(reason(&input)?, Abstention::InvalidEvidence);
    Ok(())
}
#[test]
fn reason_priority_is_stable_under_every_small_permutation() -> Result {
    let mut a = series(1)?;
    a.entitled = false;
    let mut b = series(2)?;
    b.closes[1].available_at_ms = 101;
    let c = series(3)?;
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let all = [a.clone(), b.clone(), c.clone()];
        let input = order.map(|i| all[i].clone());
        assert_eq!(reason(&input)?, Abstention::MissingEntitlement);
    }
    assert_eq!(reason(&[series(3)?])?, Abstention::UnsupportedUniverse);
    assert_eq!(
        Abstention::InsufficientHistory.code(),
        "insufficient_history"
    );
    assert!(!InvalidSelection.to_string().contains("private"));
    Ok(())
}
#[test]
fn malformed_selection_and_oversized_evidence_are_bounded() -> Result {
    let mut b = binding();
    b.max_age_ms = 0;
    assert!(EvidencePolicy::new(b, vec![selected(1)?], sessions()?, 1).is_err());
    assert!(
        EvidencePolicy::new(binding(), vec![selected(1)?, selected(1)?], sessions()?, 1).is_err()
    );
    assert!(EvidencePolicy::new(binding(), vec![selected(1)?], sessions()?, 2).is_err());
    let mut dates = sessions()?;
    dates.reverse();
    assert!(EvidencePolicy::new(binding(), vec![selected(1)?], dates, 1).is_err());
    assert_eq!(
        reason(&vec![series(1)?; MAX_INSTRUMENTS + 1])?,
        Abstention::InvalidEvidence
    );
    Ok(())
}
