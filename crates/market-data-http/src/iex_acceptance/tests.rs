//! Invented owner inputs only. No provider, clock, database or authority claim.
use super::*;
use crate::IexValue;
use crate::streaming_fixture::{Fixture, at};
use edgeagent_market_data_application::{AuthorityCatalogs, Cancellation, StreamingSession};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn fixture(bars: bool) -> TestResult<Fixture> {
    let contract = EvidenceContract::new(
        if bars {
            EvidenceKind::RawBar(BarCoverage::Intraday)
        } else {
            EvidenceKind::ShareTrade
        },
        MarketScope::Venue(Venue::Xiex),
        SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
        TimestampPrecision::Nanoseconds,
    )?;
    Fixture::profile(0, 1, contract, DeliveryCapability::Snapshots)
}
fn lane(f: &Fixture, bars: bool) -> TestResult<IexIdentityLane> {
    let mut data = f.data(1)?;
    data.delivery = DeliveryCapability::Snapshots;
    let s = f.seal(data)?;
    let plan = f.plan()?;
    let mapping = plan
        .streaming_routes(&s)?
        .first()
        .ok_or("mapping")?
        .mapping()
        .clone();
    let calendar = ReferenceVersion::new(
        ReferenceId::new(900)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        at(-1)?,
        [9; 32],
        CalendarDay::new(
            Venue::Xnas,
            SessionDate::new(1970, 1, 1)?,
            SessionHours::Regular(SessionWindow::new(at(0)?, at(3_600_000)?)?),
            "invented-v1",
        )?,
    );
    Ok(IexIdentityLane {
        mapping,
        calendar,
        window: EvidenceInterval::new(
            PreciseInstant::from_millis(at(0)?),
            PreciseInstant::from_millis(at(3_600_000)?),
        )?,
        namespace: NonZeroU64::new(7).ok_or("namespace")?,
        scope: if bars {
            IexIdentityScope::MinuteBar
        } else {
            IexIdentityScope::Trade {
                exchange: "V".into(),
                tape: "C".into(),
            }
        },
    })
}
fn profile(f: &Fixture, l: &IexIdentityLane, bars: bool) -> TestResult<IexCanonicalProfile> {
    Ok(IexCanonicalProfile::new(
        SourceId::new(1)?,
        f.contract,
        TransformationRevision::new(1)?,
        std::slice::from_ref(l),
        &if bars {
            vec![]
        } else {
            vec![IexConditionRule::new(&["@"], &[7])?]
        },
    )?)
}
fn session(f: &Fixture, p: &IexCanonicalProfile) -> TestResult<StreamingSession> {
    let mut data = f.data(1)?;
    data.delivery = DeliveryCapability::Snapshots;
    data.configuration = p.digest().clone();
    f.seal(data)
}
fn trade(f: &Fixture, l: &IexIdentityLane) -> TestResult<IexObservation> {
    Ok(IexObservation {
        mapping: l.mapping.clone(),
        contract: f.contract,
        event_at: PreciseInstant::from_unix_parts(0, 5)?,
        received_at: at(10)?,
        frames: [1, 2],
        item: 0,
        value: IexValue::Trade {
            id: 9_007_199_254_740_993,
            value: ShareTrade::new(UsdPrice::new(12345, 2)?, ShareVolume::new(7, 0)?)?,
            exchange: "V".into(),
            tape: "C".into(),
            conditions: vec!["@".into()],
        },
    })
}
fn refs<'a>(
    m: &'a ReferenceHistory<InstrumentMapping>,
    c: &'a ReferenceHistory<CalendarDay>,
) -> IexAcceptanceReferences<'a> {
    IexAcceptanceReferences {
        mappings: m,
        calendars: c,
    }
}

#[test]
fn iex_owner_inputs_derive_exact_namespace_claims_from_the_complete_profile() -> TestResult {
    let f = fixture(false)?;
    let l = lane(&f, false)?;
    let p = profile(&f, &l, false)?;
    let s = session(&f, &p)?;
    let selection = f.plan()?;
    let inputs = p.owner_inputs(&s, &selection, at(1000)?)?;
    assert_eq!(inputs.profile(), p.bytes());
    assert_eq!(inputs.claims().len(), 1);
    let claim = inputs.claims().first().ok_or("claim")?;
    assert_eq!(claim.namespace(), 7);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(claim.scope())?,
        serde_json::json!([
            "edgeagent.iex-identity-scope.v1",
            format!("{:032x}", 1),
            format!("{:032x}", 1),
            [1970, 1, 1],
            ["iex-trade", "V", "C"]
        ])
    );
    let changed = IexCanonicalProfile::new(
        SourceId::new(1)?,
        f.contract,
        TransformationRevision::new(2)?,
        std::slice::from_ref(&l),
        &[IexConditionRule::new(&["@"], &[8])?],
    )?;
    let changed_session = session(&f, &changed)?;
    let revised = changed.owner_inputs(&changed_session, &selection, at(1000)?)?;
    assert_eq!(revised.claims(), inputs.claims());
    assert_ne!(revised.bytes(), inputs.bytes());
    assert!(changed.owner_inputs(&s, &selection, at(1000)?).is_err());
    Ok(())
}
#[test]
fn iex_acceptance_trade_identity_replay_and_native_provenance_are_exact() -> TestResult {
    let f = fixture(false)?;
    let l = lane(&f, false)?;
    let p = profile(&f, &l, false)?;
    let s = session(&f, &p)?;
    let selection = f.plan()?;
    let m = ReferenceHistory::new(vec![l.mapping.clone()])?;
    let c = ReferenceHistory::new(vec![l.calendar.clone()])?;
    let accept = |input| -> TestResult<IexAcceptedObservation> {
        Ok(p.accept(
            &s,
            &selection,
            f.catalogs(),
            refs(&m, &c),
            at(100)?,
            &Cancellation::default(),
            input,
        )?)
    };
    let first: IexAcceptedObservation = accept(trade(&f, &l)?)?;
    let mut replay = trade(&f, &l)?;
    replay.received_at = at(20)?;
    replay.frames = [2, 3];
    replay.item = 1;
    let second: IexAcceptedObservation = accept(replay)?;
    let id = (7u128 << 64) | 9_007_199_254_740_993;
    assert_eq!(first.canonical().version().lineage().0.get(), id);
    assert_eq!(first.canonical().version(), second.canonical().version());
    assert_eq!(first.canonical().times().event_at().unix_parts(), (0, 5));
    assert_eq!(
        first.canonical().times().received_at(),
        PreciseInstant::from_millis(at(10)?)
    );
    assert_eq!(
        second.canonical().times().received_at(),
        PreciseInstant::from_millis(at(20)?)
    );
    assert_eq!(first.canonical().context().session_conditions().1, &[7]);
    assert_eq!(first.canonical().sequence(), None);
    assert_eq!(second.native().frames, [2, 3]);
    assert_eq!(second.native().item, 1);
    let IexValue::Trade {
        id: raw,
        conditions,
        ..
    } = &first.native().value
    else {
        return Err("native trade".into());
    };
    assert_eq!(*raw, 9_007_199_254_740_993);
    assert_eq!(conditions, &["@"]);
    assert_eq!(first.profile(), p.digest());
    assert_eq!(format!("{first:?}"), "IexAcceptedObservation(..)");
    // Source condition order is preserved even though matching is set-based.
    let grouped = IexCanonicalProfile::new(
        SourceId::new(1)?,
        f.contract,
        TransformationRevision::new(1)?,
        std::slice::from_ref(&l),
        &[
            IexConditionRule::new(&["@", "Z"], &[7, 9])?,
            IexConditionRule::new(&[], &[])?,
        ],
    )?;
    let grouped_session = session(&f, &grouped)?;
    let mut input = trade(&f, &l)?;
    if let IexValue::Trade { conditions, .. } = &mut input.value {
        *conditions = vec!["Z".into(), "@".into()];
    }
    let result = grouped.accept(
        &grouped_session,
        &selection,
        f.catalogs(),
        refs(&m, &c),
        at(100)?,
        &Cancellation::default(),
        input,
    )?;
    assert_eq!(result.canonical().context().session_conditions().1, &[7, 9]);
    assert!(
        matches!(&result.native().value,IexValue::Trade{conditions,..} if conditions==&["Z","@"])
    );
    let mut input = trade(&f, &l)?;
    if let IexValue::Trade { conditions, .. } = &mut input.value {
        conditions.clear();
    }
    let result = grouped.accept(
        &grouped_session,
        &selection,
        f.catalogs(),
        refs(&m, &c),
        at(100)?,
        &Cancellation::default(),
        input,
    )?;
    assert!(
        result
            .canonical()
            .context()
            .session_conditions()
            .1
            .is_empty()
    );
    Ok(())
}
#[test]
fn iex_acceptance_minute_bars_preserve_raw_coverage_count_vwap_and_bounds() -> TestResult {
    let f = fixture(true)?;
    let l = lane(&f, true)?;
    let p = profile(&f, &l, true)?;
    let s = session(&f, &p)?;
    let selection = f.plan()?;
    let m = ReferenceHistory::new(vec![l.mapping.clone()])?;
    let c = ReferenceHistory::new(vec![l.calendar.clone()])?;
    let row = || -> TestResult<IexObservation> {
        Ok(IexObservation {
            mapping: l.mapping.clone(),
            contract: f.contract,
            event_at: PreciseInstant::from_millis(at(0)?),
            received_at: at(60_000)?,
            frames: [1, 2],
            item: 2,
            value: IexValue::MinuteBar {
                value: RawBar::new(
                    EvidenceInterval::new(
                        PreciseInstant::from_millis(at(0)?),
                        PreciseInstant::from_millis(at(60_000)?),
                    )?,
                    BarCoverage::Intraday,
                    [
                        UsdPrice::new(10, 0)?,
                        UsdPrice::new(12, 0)?,
                        UsdPrice::new(9, 0)?,
                        UsdPrice::new(11, 0)?,
                    ],
                    ShareVolume::new(7, 0)?,
                )?,
                trades: 42,
                vwap: UsdPrice::new(1101, 2)?,
            },
        })
    };
    let accepted = p.accept(
        &s,
        &selection,
        f.catalogs(),
        refs(&m, &c),
        at(60_001)?,
        &Cancellation::default(),
        row()?,
    )?;
    assert_eq!(
        accepted.canonical().version().lineage().0.get(),
        7u128 << 64
    );
    assert!(
        matches!(accepted.canonical().value(),Some(ObservationValue::Bar(b)) if b.bucket().1==BarCoverage::Intraday)
    );
    assert!(
        matches!(accepted.native().value,IexValue::MinuteBar{trades:42,vwap,..} if vwap==UsdPrice::new(1101,2)?)
    );
    let mut invalid = row()?;
    invalid.event_at = PreciseInstant::from_unix_parts(0, 1)?;
    assert!(
        p.accept(
            &s,
            &selection,
            f.catalogs(),
            refs(&m, &c),
            at(60_001)?,
            &Cancellation::default(),
            invalid
        )
        .is_err()
    );
    let mut invalid = row()?;
    invalid.received_at = at(59_999)?;
    assert!(
        p.accept(
            &s,
            &selection,
            f.catalogs(),
            refs(&m, &c),
            at(60_001)?,
            &Cancellation::default(),
            invalid
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn iex_acceptance_rejects_unknown_semantics_stale_references_and_authority() -> TestResult {
    let f = fixture(false)?;
    let l = lane(&f, false)?;
    let p = profile(&f, &l, false)?;
    let s = session(&f, &p)?;
    let selection = f.plan()?;
    let m = ReferenceHistory::new(vec![l.mapping.clone()])?;
    let c = ReferenceHistory::new(vec![l.calendar.clone()])?;
    for mode in 0..8 {
        let mut input = trade(&f, &l)?;
        match mode {
            0 => {
                if let IexValue::Trade { conditions, .. } = &mut input.value {
                    *conditions = vec!["?".into()];
                }
            }
            1 => {
                if let IexValue::Trade { conditions, .. } = &mut input.value {
                    *conditions = vec!["@".into(), "@".into()];
                }
            }
            2 => {
                if let IexValue::Trade { exchange, .. } = &mut input.value {
                    *exchange = "unknown".into();
                }
            }
            3 => input.frames = [0, 1],
            4 => input.item = 1024,
            5 => input.received_at = at(101)?,
            6 => input.event_at = PreciseInstant::from_millis(at(3_600_000)?),
            _ => {
                if let IexValue::Trade { conditions, .. } = &mut input.value {
                    conditions.clear();
                }
            }
        }
        assert!(
            p.accept(
                &s,
                &selection,
                f.catalogs(),
                refs(&m, &c),
                at(100)?,
                &Cancellation::default(),
                input
            )
            .is_err()
        );
    }
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        p.accept(
            &s,
            &selection,
            f.catalogs(),
            refs(&m, &c),
            at(100)?,
            &cancel,
            trade(&f, &l)?
        )
        .is_err()
    );
    let revoked = f.revoked()?;
    let catalogs = AuthorityCatalogs {
        rights: &revoked,
        providers: &f.providers,
    };
    assert!(
        p.accept(
            &s,
            &selection,
            catalogs,
            refs(&m, &c),
            at(100)?,
            &Cancellation::default(),
            trade(&f, &l)?
        )
        .is_err()
    );
    let newer = ReferenceVersion::new(
        l.calendar.id(),
        ReferenceRevision::new(2)?,
        l.calendar.source(),
        at(0)?,
        [8; 32],
        l.calendar.value().clone(),
    );
    let changed = ReferenceHistory::new(vec![l.calendar.clone(), newer])?;
    assert_eq!(
        p.accept(
            &s,
            &selection,
            f.catalogs(),
            refs(&m, &changed),
            at(100)?,
            &Cancellation::default(),
            trade(&f, &l)?
        )
        .err(),
        Some(IexAcceptanceError::Reference)
    );
    let mut wrong = f.data(1)?;
    wrong.delivery = DeliveryCapability::Snapshots;
    assert_eq!(
        p.accept(
            &f.seal(wrong)?,
            &selection,
            f.catalogs(),
            refs(&m, &c),
            at(100)?,
            &Cancellation::default(),
            trade(&f, &l)?
        )
        .err(),
        Some(IexAcceptanceError::Binding)
    );
    Ok(())
}
#[test]
fn iex_acceptance_profile_bytes_bind_complete_inputs_and_reject_ambiguity() -> TestResult {
    let f = fixture(false)?;
    let l = lane(&f, false)?;
    let t = TransformationRevision::new(1)?;
    let rules = [
        IexConditionRule::new(&["@"], &[7])?,
        IexConditionRule::new(&["Z"], &[9])?,
    ];
    let mut second = l.clone();
    second.namespace = NonZeroU64::new(8).ok_or("namespace")?;
    second.scope = IexIdentityScope::Trade {
        exchange: "Y".into(),
        tape: "C".into(),
    };
    let p = IexCanonicalProfile::new(
        SourceId::new(1)?,
        f.contract,
        t,
        &[l.clone(), second.clone()],
        &rules,
    )?;
    let reversed = IexCanonicalProfile::new(
        SourceId::new(1)?,
        f.contract,
        t,
        &[second, l.clone()],
        &[rules[1].clone(), rules[0].clone()],
    )?;
    assert_eq!(p.bytes(), reversed.bytes());
    assert_eq!(p.digest(), reversed.digest());
    assert!(
        IexCanonicalProfile::new(
            SourceId::new(1)?,
            f.contract,
            t,
            &[l.clone(), l.clone()],
            &rules
        )
        .is_err()
    );
    assert!(
        IexCanonicalProfile::new(
            SourceId::new(1)?,
            f.contract,
            t,
            std::slice::from_ref(&l),
            &[rules[0].clone(), rules[0].clone()]
        )
        .is_err()
    );
    assert!(IexConditionRule::new(&["Z", "@"], &[1]).is_err());
    assert!(IexConditionRule::new(&["@"], &[2, 1]).is_err());
    let mut changed = l.clone();
    let old = &l.calendar;
    changed.calendar = ReferenceVersion::new(
        old.id(),
        old.revision(),
        old.source(),
        old.known_at(),
        *old.checksum(),
        CalendarDay::new(
            Venue::Xnas,
            SessionDate::new(1970, 1, 1)?,
            old.value().hours(),
            "invented-v2",
        )?,
    );
    let changed = IexCanonicalProfile::new(SourceId::new(1)?, f.contract, t, &[changed], &rules)?;
    let original = IexCanonicalProfile::new(
        SourceId::new(1)?,
        f.contract,
        t,
        std::slice::from_ref(&l),
        &rules,
    )?;
    assert_ne!(changed.digest(), original.digest());
    for text in [
        format!("{p:?}"),
        format!("{l:?}"),
        format!("{:?}", rules[0]),
        format!("{:?}", l.scope),
    ] {
        assert!(!text.contains("SYNTH1"));
        assert!(!text.contains("invented-v1"));
    }
    Ok(())
}
