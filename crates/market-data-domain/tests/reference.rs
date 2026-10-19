mod fixtures;

use edgeagent_market_data_domain::*;
use fixtures::*;
use std::num::NonZeroU64;

#[test]
fn ticker_reuse_boundaries_and_venue_resolution_fail_closed() -> TestResult {
    let known = utc(1, 1, 0, 0)?;
    let cutoff = utc(6, 1, 0, 0)?;
    let symbol = Symbol::new("INVENTED")?;
    let old = version(1, 1, known, mapping(1, date(1, 1)?, date(3, 1)?)?)?;
    let new = version(2, 1, known, mapping(2, date(3, 1)?, date(5, 1)?)?)?;
    let history = ReferenceHistory::new(vec![new.clone(), old])?;
    for (effective, expected) in [(date(1, 1)?, 1), (date(2, 28)?, 1), (date(3, 1)?, 2)] {
        let found = history.resolve_mapping(&symbol, Venue::Xnys, effective, cutoff)?;
        assert_eq!(
            found.value().identities(),
            (InstrumentId::new(expected)?, ListingId::new(expected)?)
        );
        assert_eq!(found.value().binding().0, &symbol);
        assert_eq!(
            found.value().binding().2.bounds(),
            if expected == 1 {
                (date(1, 1)?, date(3, 1)?)
            } else {
                (date(3, 1)?, date(5, 1)?)
            }
        );
    }
    assert_eq!(
        history.resolve_mapping(&symbol, Venue::Xnys, date(5, 1)?, cutoff),
        Err(ReferenceError::Missing)
    );
    for venue in [Venue::Xnas, Venue::Arcx] {
        assert_eq!(
            history.resolve_mapping(&symbol, venue, date(3, 1)?, cutoff),
            Err(ReferenceError::Missing)
        );
    }
    let overlapping = ReferenceHistory::new(vec![
        new,
        version(3, 1, known, mapping(3, date(2, 1)?, date(4, 1)?)?)?,
    ])?;
    assert_eq!(
        overlapping.resolve_mapping(&symbol, Venue::Xnys, date(3, 1)?, cutoff),
        Err(ReferenceError::Ambiguous)
    );
    Ok(())
}

#[test]
fn corrections_are_selected_before_effective_filtering_at_inclusive_cutoff() -> TestResult {
    let known = utc(1, 1, 0, 0)?;
    let corrected = utc(4, 1, 0, 0)?;
    let original = version(1, 1, known, mapping(1, date(1, 1)?, date(5, 1)?)?)?;
    let correction = version(1, 2, corrected, mapping(1, date(3, 1)?, date(5, 1)?)?)?;
    let history = ReferenceHistory::new(vec![correction, original.clone()])?;
    let symbol = Symbol::new("INVENTED")?;
    assert_eq!(
        history.resolve_mapping(&symbol, Venue::Xnys, date(2, 1)?, known)?,
        &original
    );
    assert_eq!(
        history.resolve_mapping(&symbol, Venue::Xnys, date(2, 1)?, corrected),
        Err(ReferenceError::Missing)
    );
    let before = UtcInstant::from_unix_millis(known.unix_millis() - 1)?;
    assert_eq!(
        history.resolve_mapping(&symbol, Venue::Xnys, date(2, 1)?, before),
        Err(ReferenceError::Missing)
    );
    assert_eq!(original.source(), SourceId::new(7)?);
    assert_eq!(original.known_at(), known);
    assert_eq!(original.checksum(), &[91; 32]);
    assert_eq!(original.id(), ReferenceId::new(1)?);
    assert_eq!(original.revision(), ReferenceRevision::new(1)?);
    Ok(())
}

#[test]
fn revision_history_is_bounded_order_independent_and_rejects_contradictions() -> TestResult {
    let a = utc(1, 1, 0, 0)?;
    let b = utc(1, 2, 0, 0)?;
    let first = version(1, 1, a, ())?;
    assert_eq!(
        ReferenceHistory::new(vec![first.clone(), first.clone()]),
        Err(ReferenceError::RevisionConflict)
    );
    assert_eq!(
        ReferenceHistory::new(vec![version(1, 2, a, ())?, version(1, 1, b, ())?]),
        Err(ReferenceError::RevisionConflict)
    );
    assert_eq!(
        ReferenceHistory::new(vec![first.clone(); 4097]),
        Err(ReferenceError::Capacity)
    );
    let mut versions = Vec::new();
    for id in 1..=4096 {
        versions.push(version(id, 1, a, ())?);
    }
    assert_eq!(ReferenceHistory::new(versions)?.at(a).count(), 4096);
    assert_eq!(ReferenceHistory::<()>::new(vec![])?.at(a).count(), 0);
    let versions = vec![version(2, 1, a, ())?, version(1, 3, b, ())?, first];
    let history = ReferenceHistory::new(versions.clone())?;
    assert_eq!(
        history,
        ReferenceHistory::new(versions.into_iter().rev().collect())?
    );
    assert_eq!(
        history
            .at(a)
            .map(|v| v.revision().get())
            .collect::<Vec<_>>(),
        vec![1, 1]
    );
    assert_eq!(
        history
            .at(b)
            .map(|v| v.revision().get())
            .collect::<Vec<_>>(),
        vec![3, 1]
    );
    // Equal receipt times are unambiguous because revisions establish order.
    assert_eq!(
        ReferenceHistory::new(vec![version(1, 1, a, ())?, version(1, 2, a, ())?])?
            .at(a)
            .map(|v| v.revision().get())
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert_eq!(ReferenceId::new(0), Err(ValueError::ZeroIdentity));
    assert_eq!(ReferenceRevision::new(0), Err(ValueError::ZeroIdentity));
    Ok(())
}

#[test]
fn explicit_calendar_preserves_dst_early_closes_holidays_and_unknown_days() -> TestResult {
    let known = utc(1, 1, 0, 0)?;
    let before_dst = window(3, 6, 14, 21, 0)?;
    let after_dst = window(3, 9, 13, 20, 0)?;
    let after_fall = window(11, 2, 14, 21, 0)?;
    let early = window(11, 27, 14, 18, 0)?;
    let history = ReferenceHistory::new(vec![
        version(1, 1, known, day(3, 6, SessionHours::Regular(before_dst))?)?,
        version(2, 1, known, day(3, 9, SessionHours::Regular(after_dst))?)?,
        version(3, 1, known, day(11, 2, SessionHours::Regular(after_fall))?)?,
        version(4, 1, known, day(11, 26, SessionHours::Closed)?)?,
        version(5, 1, known, day(11, 27, SessionHours::EarlyClose(early))?)?,
    ])?;
    for (m, d, expected) in [
        (3, 6, SessionHours::Regular(before_dst)),
        (3, 9, SessionHours::Regular(after_dst)),
        (11, 2, SessionHours::Regular(after_fall)),
        (11, 26, SessionHours::Closed),
        (11, 27, SessionHours::EarlyClose(early)),
    ] {
        let found = history
            .resolve_day(Venue::Xnys, date(m, d)?, known)?
            .value();
        assert_eq!(found.hours(), expected);
        assert_eq!(found.timezone(), ("America/New_York", "synthetic-2026a"));
    }
    assert_eq!(before_dst.bounds(), (utc(3, 6, 14, 30)?, utc(3, 6, 21, 0)?));
    assert_eq!(after_dst.bounds(), (utc(3, 9, 13, 30)?, utc(3, 9, 20, 0)?));
    assert_eq!(early.bounds(), (utc(11, 27, 14, 30)?, utc(11, 27, 18, 0)?));
    // No implicit weekend, weekday or out-of-range coverage.
    for (m, d) in [(3, 7), (3, 10), (12, 31)] {
        assert_eq!(
            history.resolve_day(Venue::Xnys, date(m, d)?, known),
            Err(ReferenceError::Missing)
        );
    }
    assert_eq!(
        history.resolve_day(Venue::Xnas, date(3, 9)?, known),
        Err(ReferenceError::Missing)
    );
    let notice = utc(3, 5, 0, 0)?;
    let corrections = ReferenceHistory::new(vec![
        version(1, 1, known, day(3, 6, SessionHours::Regular(before_dst))?)?,
        version(1, 2, notice, day(3, 6, SessionHours::Closed)?)?,
    ])?;
    assert_eq!(
        corrections
            .resolve_day(Venue::Xnys, date(3, 6)?, known)?
            .value()
            .hours(),
        SessionHours::Regular(before_dst)
    );
    assert_eq!(
        corrections
            .resolve_day(Venue::Xnys, date(3, 6)?, notice)?
            .value()
            .hours(),
        SessionHours::Closed
    );
    let conflict = ReferenceHistory::new(vec![
        version(1, 1, known, day(3, 6, SessionHours::Closed)?)?,
        version(2, 1, known, day(3, 6, SessionHours::Regular(before_dst))?)?,
    ])?;
    assert_eq!(
        conflict.resolve_day(Venue::Xnys, date(3, 6)?, known),
        Err(ReferenceError::Ambiguous)
    );
    assert_eq!(
        corrections.resolve_day(Venue::Xnys, date(3, 6)?, UtcInstant::from_unix_millis(0)?),
        Err(ReferenceError::Missing)
    );
    Ok(())
}

#[test]
fn action_effective_dates_do_not_establish_knowledge_or_adjust_raw_prices() -> TestResult {
    let instrument = InstrumentId::new(1)?;
    let known = utc(2, 1, 0, 0)?;
    let learned = utc(4, 1, 0, 0)?;
    let effective = date(3, 1)?;
    let forward = ActionKind::Split {
        before: NonZeroU64::new(1).ok_or("zero fixture")?,
        after: NonZeroU64::new(4).ok_or("zero fixture")?,
    };
    let reverse = ActionKind::Split {
        before: NonZeroU64::new(10).ok_or("zero fixture")?,
        after: NonZeroU64::new(1).ok_or("zero fixture")?,
    };
    let raw = UsdPrice::new(12345, 2)?;
    let history = ReferenceHistory::new(vec![
        version(
            1,
            1,
            known,
            CorporateAction::new(instrument, effective, forward),
        )?,
        version(
            1,
            2,
            learned,
            CorporateAction::new(instrument, effective, reverse),
        )?,
        version(
            2,
            1,
            learned,
            CorporateAction::new(
                instrument,
                effective,
                ActionKind::CashDividend(UsdPrice::new(25, 2)?),
            ),
        )?,
    ])?;
    assert_eq!(
        history
            .actions_on(instrument, effective, known)?
            .iter()
            .map(|v| v.value().kind())
            .collect::<Vec<_>>(),
        vec![forward]
    );
    assert_eq!(
        history
            .actions_on(instrument, effective, learned)?
            .iter()
            .map(|v| v.value().kind())
            .collect::<Vec<_>>(),
        vec![reverse, ActionKind::CashDividend(UsdPrice::new(25, 2)?)]
    );
    assert!(
        history
            .actions_on(instrument, date(2, 28)?, learned)?
            .is_empty()
    );
    assert!(
        history
            .actions_on(InstrumentId::new(2)?, effective, learned)?
            .is_empty()
    );
    assert!(
        history
            .actions_on(instrument, effective, utc(1, 31, 0, 0)?)?
            .is_empty()
    );
    assert_eq!(raw.decimal_parts(), (12345, 2));
    let withdrawn = ReferenceHistory::new(vec![
        version(
            1,
            1,
            known,
            CorporateAction::new(instrument, effective, forward),
        )?,
        version(
            1,
            2,
            learned,
            CorporateAction::new(instrument, date(3, 2)?, ActionKind::Cancelled),
        )?,
    ])?;
    assert_eq!(withdrawn.actions_on(instrument, effective, known)?.len(), 1);
    assert!(
        withdrawn
            .actions_on(instrument, effective, learned)?
            .is_empty()
    );
    assert!(
        withdrawn
            .actions_on(instrument, date(3, 2)?, learned)?
            .is_empty()
    );
    let unsupported = ReferenceHistory::new(vec![version(
        1,
        1,
        learned,
        CorporateAction::new(instrument, effective, ActionKind::Unsupported),
    )?])?;
    assert_eq!(
        unsupported.actions_on(instrument, effective, learned),
        Err(ReferenceError::UnsupportedAction)
    );
    Ok(())
}

#[test]
fn construction_rejects_invalid_tokens_ranges_and_session_windows_without_payload_diagnostics()
-> TestResult {
    for value in ["", "not a symbol", "vendor/secret", "é", &"x".repeat(33)] {
        assert_eq!(Symbol::new(value), Err(ReferenceError::InvalidToken));
    }
    assert_eq!(Symbol::new("BRK.B-_1")?.as_str(), "BRK.B-_1");
    assert!(Symbol::new(&"x".repeat(32)).is_ok());
    let d = date(3, 6)?;
    assert_eq!(DateRange::new(d, d), Err(ReferenceError::InvalidInterval));
    assert_eq!(
        DateRange::new(date(3, 7)?, d),
        Err(ReferenceError::InvalidInterval)
    );
    let open = utc(3, 6, 14, 30)?;
    for close in [open, utc(3, 6, 14, 29)?, utc(3, 7, 21, 0)?] {
        assert_eq!(
            SessionWindow::new(open, close),
            Err(ReferenceError::InvalidInterval)
        );
    }
    assert_eq!(
        CalendarDay::new(
            Venue::Xnys,
            date(3, 7)?,
            SessionHours::Regular(window(3, 6, 14, 21, 0)?),
            "synthetic"
        ),
        Err(ReferenceError::InvalidInterval)
    );
    for revision in ["", "bad revision", &"x".repeat(33)] {
        assert_eq!(
            CalendarDay::new(Venue::Xnys, d, SessionHours::Closed, revision),
            Err(ReferenceError::InvalidToken)
        );
    }
    let symbol = Symbol::new("SENTINEL-SECRET")?;
    let reference = version(1, 1, open, symbol.clone())?;
    assert!(!format!("{symbol:?} {reference:?}").contains("SENTINEL"));
    assert!(!format!("{reference:?}").contains("91"));
    assert_eq!(
        format!("{:?}", ActionKind::CashDividend(UsdPrice::new(98765, 2)?)),
        "CashDividend(..)"
    );
    fn portable<T: Send + Sync + Clone + Eq + std::fmt::Debug>() {}
    portable::<ReferenceHistory<InstrumentMapping>>();
    portable::<ReferenceHistory<CalendarDay>>();
    portable::<ReferenceHistory<CorporateAction>>();
    Ok(())
}
