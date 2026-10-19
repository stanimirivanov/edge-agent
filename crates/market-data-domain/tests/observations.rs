use edgeagent_market_data_domain::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn send_sync<T: Send + Sync + Clone + std::fmt::Debug + Eq>() {}
fn at(seconds: i64, nanos: u32) -> Result<PreciseInstant, ObservationError> {
    PreciseInstant::from_unix_parts(seconds, nanos)
}
fn ms(value: i64) -> Result<UtcInstant, ValueError> {
    UtcInstant::from_unix_millis(value)
}
fn semantics(revision: u64) -> Result<SemanticReference, ValueError> {
    Ok(SemanticReference::new(
        ReferenceId::new(10)?,
        ReferenceRevision::new(revision)?,
    ))
}
fn contract(kind: EvidenceKind) -> Result<EvidenceContract, Box<dyn std::error::Error>> {
    Ok(EvidenceContract::new(
        kind,
        if kind == EvidenceKind::IndexLevel {
            MarketScope::IndexPublisher
        } else {
            MarketScope::Consolidated
        },
        semantics(1)?,
        TimestampPrecision::Nanoseconds,
    )?)
}
fn context(
    source: u128,
    contract: EvidenceContract,
    delivery: DeliveryCapability,
) -> Result<ObservationContext, Box<dyn std::error::Error>> {
    Ok(ObservationContext::new(
        InstrumentId::new(9)?,
        ProviderCapability::new(
            SourceId::new(source)?,
            contract,
            delivery,
            KnowledgeCapability::CaptureOnly,
        ),
        TransformationRevision::new(1)?,
        SessionDate::new(1970, 1, 1)?,
        &[1, 65530],
    )?)
}
fn version(
    revision: u64,
    previous: Option<u64>,
) -> Result<ObservationVersion, Box<dyn std::error::Error>> {
    Ok(ObservationVersion::new(
        ObservationId::new(4)?,
        ObservationRevision::new(revision)?,
        previous.map(ObservationRevision::new).transpose()?,
    )?)
}
fn times() -> Result<EvidenceTimes, ObservationError> {
    EvidenceTimes::new(at(2, 0)?, Some(at(2, 1)?), None, at(3, 0)?)
}
fn bar(coverage: BarCoverage) -> Result<RawBar, Box<dyn std::error::Error>> {
    Ok(RawBar::new(
        EvidenceInterval::new(at(0, 0)?, at(2, 0)?)?,
        coverage,
        [
            UsdPrice::new(110, 0)?,
            UsdPrice::new(120, 0)?,
            UsdPrice::new(100, 0)?,
            UsdPrice::new(115, 0)?,
        ],
        ShareVolume::new(0, 0)?,
    )?)
}
fn spec(
    known_at: i64,
) -> Result<ReferenceVersion<OptionSpecification>, Box<dyn std::error::Error>> {
    let schedule = OptionSchedule::new(
        ms(0)?,
        ms(8000)?,
        ms(9000)?,
        ms(9000)?,
        ms(10000)?,
        OptionSchedule::timezone_rules().1,
    )?;
    let terms = OptionTerms::new(
        OptionKind::Call,
        OptionPoints::new(100, 0)?,
        OptionPoints::new(5, 2)?,
        UsdPerPoint::new(100, 0)?,
        ExerciseStyle::European,
        SettlementStyle::Cash,
    )?;
    Ok(ReferenceVersion::new(
        ReferenceId::new(20)?,
        ReferenceRevision::new(1)?,
        SourceId::new(8)?,
        ms(known_at)?,
        [7; 32],
        OptionSpecification::new(
            InstrumentId::new(9)?,
            Underlying::Index(InstrumentId::new(8)?),
            terms,
            schedule,
            EffectiveInterval::new(ms(0)?, ms(10000)?)?,
        )?,
    ))
}

#[test]
fn precise_times_preserve_pre_epoch_and_nanosecond_boundaries_without_rounding() -> TestResult {
    for (seconds, nanos) in [
        (-62_135_596_800, 0),
        (-1, 999_999_999),
        (0, 0),
        (0, 1),
        (253_402_300_799, 999_999_999),
    ] {
        let value = at(seconds, nanos)?;
        assert_eq!(value.unix_parts(), (seconds, u64::from(nanos)));
    }
    for seconds in [i64::MIN, i64::MAX, -62_135_596_801, 253_402_300_800] {
        assert_eq!(at(seconds, 0), Err(ObservationError::Timestamp));
    }
    assert_eq!(at(0, 1_000_000_000), Err(ObservationError::Timestamp));
    assert!(at(-1, 999_999_999)? < at(0, 0)?);
    assert!(at(0, 999_999)? < at(0, 1_000_000)?);
    assert_eq!(
        PreciseInstant::from_millis(ms(-1)?).unix_parts(),
        (-1, 999_000_000)
    );
    assert_eq!(
        EvidenceInterval::new(at(0, 1)?, at(0, 1)?),
        Err(ObservationError::TimeOrder)
    );
    assert_eq!(
        EvidenceInterval::new(at(0, 2)?, at(0, 1)?),
        Err(ObservationError::TimeOrder)
    );
    let event = at(0, 10)?;
    for (observed, published, received) in [
        (Some(at(0, 9)?), None, at(0, 11)?),
        (Some(at(0, 11)?), Some(event), at(0, 12)?),
        (None, Some(at(0, 11)?), event),
        (None, None, at(0, 9)?),
    ] {
        assert_eq!(
            EvidenceTimes::new(event, observed, published, received),
            Err(ObservationError::TimeOrder)
        );
    }
    let t = EvidenceTimes::new(event, None, None, event)?;
    assert_eq!(t.provider_times(), (None, None));
    assert_eq!(t.received_at(), event);
    Ok(())
}

#[test]
fn raw_bars_and_trades_reject_impossible_values_and_preserve_coverage() -> TestResult {
    let interval = EvidenceInterval::new(at(0, 0)?, at(2, 0)?)?;
    for values in [[3, 2, 1, 1], [1, 2, 3, 1], [1, 2, 1, 3], [1, 3, 2, 2]] {
        let [a, b, c, d] = values;
        assert_eq!(
            RawBar::new(
                interval,
                BarCoverage::ProviderDay,
                [
                    UsdPrice::new(a, 0)?,
                    UsdPrice::new(b, 0)?,
                    UsdPrice::new(c, 0)?,
                    UsdPrice::new(d, 0)?
                ],
                ShareVolume::new(0, 0)?
            ),
            Err(ObservationError::Ohlc)
        );
    }
    let daily = bar(BarCoverage::ProviderDay)?;
    assert_eq!(daily.bucket(), (interval, BarCoverage::ProviderDay));
    assert_eq!(daily.values().1.decimal_parts(), (0, 0));
    assert_eq!(
        ShareTrade::new(UsdPrice::new(1, 0)?, ShareVolume::new(0, 0)?),
        Err(ObservationError::TradeSize)
    );
    assert_eq!(IndexPoints::new(0, 0), Err(ValueError::InvalidSign));
    let trade = ShareTrade::new(UsdPrice::new(1, 0)?, ShareVolume::new(1, 6)?)?;
    assert_eq!(trade.values().1.decimal_parts(), (1, 6));
    for coverage in [
        BarCoverage::ProviderDay,
        BarCoverage::CoreSession,
        BarCoverage::Intraday,
    ] {
        let record = CanonicalObservation::new(
            context(
                1,
                contract(EvidenceKind::RawBar(coverage))?,
                DeliveryCapability::Snapshots,
            )?,
            version(1, None)?,
            times()?,
            None,
            Some(ObservationValue::Bar(bar(coverage)?)),
        )?;
        assert_eq!(record.value(), Some(ObservationValue::Bar(bar(coverage)?)));
    }
    let ctx = context(
        1,
        contract(EvidenceKind::RawBar(BarCoverage::ProviderDay))?,
        DeliveryCapability::Snapshots,
    )?;
    let early = EvidenceTimes::new(at(-1, 999_999_999)?, None, None, at(3, 0)?)?;
    assert_eq!(
        CanonicalObservation::new(
            ctx.clone(),
            version(1, None)?,
            early,
            None,
            Some(ObservationValue::Bar(daily))
        ),
        Err(ObservationError::TimeOrder)
    );
    // A provider may label a completed aggregate by bucket start. Preserve it,
    // but never admit a still-forming bucket as a completed raw bar.
    let start_label = EvidenceTimes::new(at(0, 0)?, None, None, at(2, 0)?)?;
    let completed = CanonicalObservation::new(
        ctx.clone(),
        version(1, None)?,
        start_label,
        None,
        Some(ObservationValue::Bar(daily)),
    )?;
    assert_eq!(completed.times().event_at(), at(0, 0)?);
    let forming = EvidenceTimes::new(at(0, 0)?, None, None, at(1, 999_999_999)?)?;
    assert_eq!(
        CanonicalObservation::new(
            ctx.clone(),
            version(1, None)?,
            forming,
            None,
            Some(ObservationValue::Bar(daily))
        ),
        Err(ObservationError::TimeOrder)
    );
    assert_eq!(
        CanonicalObservation::new(
            ctx,
            version(1, None)?,
            times()?,
            None,
            Some(ObservationValue::Trade(trade))
        ),
        Err(ObservationError::Contract)
    );
    Ok(())
}

#[test]
fn capabilities_match_semantics_not_vendor_names_or_index_proxies() -> TestResult {
    let kinds = [
        EvidenceKind::RawBar(BarCoverage::ProviderDay),
        EvidenceKind::RawBar(BarCoverage::CoreSession),
        EvidenceKind::RawBar(BarCoverage::Intraday),
        EvidenceKind::ShareTrade,
        EvidenceKind::IndexLevel,
        EvidenceKind::OptionSnapshot,
    ];
    for kind in kinds {
        let wanted = contract(kind)?;
        for source in [1, 2] {
            let offered = ProviderCapability::new(
                SourceId::new(source)?,
                wanted,
                DeliveryCapability::Sequenced,
                KnowledgeCapability::HistoricalVersions,
            );
            assert_eq!(offered.source().get(), source);
            for delivery in [DeliveryCapability::Snapshots, DeliveryCapability::Sequenced] {
                for knowledge in [
                    KnowledgeCapability::CaptureOnly,
                    KnowledgeCapability::HistoricalVersions,
                ] {
                    assert_eq!(offered.require(wanted, delivery, knowledge), Ok(()));
                }
            }
            for other in kinds {
                let result = offered.require(
                    contract(other)?,
                    DeliveryCapability::Snapshots,
                    KnowledgeCapability::CaptureOnly,
                );
                assert_eq!(result.is_ok(), kind == other);
            }
        }
        let limited = ProviderCapability::new(
            SourceId::new(1)?,
            wanted,
            DeliveryCapability::Snapshots,
            KnowledgeCapability::CaptureOnly,
        );
        assert_eq!(
            limited.require(
                wanted,
                DeliveryCapability::Sequenced,
                KnowledgeCapability::CaptureOnly
            ),
            Err(ObservationError::UnsupportedCapability)
        );
        assert_eq!(
            limited.require(
                wanted,
                DeliveryCapability::Snapshots,
                KnowledgeCapability::HistoricalVersions
            ),
            Err(ObservationError::UnsupportedCapability)
        );
        let changed_rules = EvidenceContract::new(
            kind,
            wanted.shape().1,
            semantics(2)?,
            TimestampPrecision::Nanoseconds,
        )?;
        assert_eq!(
            limited.require(
                changed_rules,
                DeliveryCapability::Snapshots,
                KnowledgeCapability::CaptureOnly
            ),
            Err(ObservationError::UnsupportedCapability)
        );
        let changed_precision = EvidenceContract::new(
            kind,
            wanted.shape().1,
            semantics(1)?,
            TimestampPrecision::Microseconds,
        )?;
        assert_eq!(
            limited.require(
                changed_precision,
                DeliveryCapability::Snapshots,
                KnowledgeCapability::CaptureOnly
            ),
            Err(ObservationError::UnsupportedCapability)
        );
    }
    Ok(())
}

#[test]
fn invalid_market_scopes_and_implicit_consolidation_are_rejected() -> TestResult {
    assert_eq!(
        EvidenceContract::new(
            EvidenceKind::IndexLevel,
            MarketScope::Consolidated,
            semantics(1)?,
            TimestampPrecision::Nanoseconds
        ),
        Err(ObservationError::Contract)
    );
    assert_eq!(
        EvidenceContract::new(
            EvidenceKind::ShareTrade,
            MarketScope::IndexPublisher,
            semantics(1)?,
            TimestampPrecision::Nanoseconds
        ),
        Err(ObservationError::Contract)
    );
    assert_eq!(
        EvidenceContract::new(
            EvidenceKind::OptionSnapshot,
            MarketScope::Venue(Venue::Arcx),
            semantics(1)?,
            TimestampPrecision::Nanoseconds
        ),
        Err(ObservationError::Contract)
    );
    let venue = EvidenceContract::new(
        EvidenceKind::ShareTrade,
        MarketScope::Venue(Venue::Xnys),
        semantics(1)?,
        TimestampPrecision::Nanoseconds,
    )?;
    let provider = ProviderCapability::new(
        SourceId::new(1)?,
        venue,
        DeliveryCapability::Snapshots,
        KnowledgeCapability::CaptureOnly,
    );
    assert_eq!(
        provider.require(
            contract(EvidenceKind::ShareTrade)?,
            DeliveryCapability::Snapshots,
            KnowledgeCapability::CaptureOnly
        ),
        Err(ObservationError::UnsupportedCapability)
    );
    Ok(())
}

#[test]
fn revisions_cancellations_sequences_and_conditions_remain_explicit() -> TestResult {
    assert_eq!(ObservationId::new(0), Err(ValueError::ZeroIdentity));
    assert_eq!(ObservationRevision::new(0), Err(ValueError::ZeroIdentity));
    for (revision, previous) in [(2, None), (1, Some(1)), (2, Some(2)), (2, Some(3))] {
        assert!(version(revision, previous).is_err());
    }
    let ctx = context(
        1,
        contract(EvidenceKind::IndexLevel)?,
        DeliveryCapability::Sequenced,
    )?;
    let value = Some(ObservationValue::IndexLevel(IndexPoints::new(100, 0)?));
    assert_eq!(
        CanonicalObservation::new(ctx.clone(), version(1, None)?, times()?, None, value),
        Err(ObservationError::Sequence)
    );
    assert_eq!(
        CanonicalObservation::new(ctx.clone(), version(1, None)?, times()?, Some(0), None),
        Err(ObservationError::Revision)
    );
    let original =
        CanonicalObservation::new(ctx.clone(), version(1, None)?, times()?, Some(0), value)?;
    let correction = CanonicalObservation::new(
        ctx.clone(),
        version(3, Some(1))?,
        times()?,
        Some(1),
        Some(ObservationValue::IndexLevel(IndexPoints::new(101, 0)?)),
    )?;
    let cancel = CanonicalObservation::new(
        ctx.clone(),
        version(4, Some(3))?,
        times()?,
        Some(u64::MAX),
        None,
    )?;
    assert_eq!(original.sequence(), Some(0));
    assert_eq!(original.value(), value);
    assert_ne!(original, correction);
    assert_eq!(
        correction.version().lineage().2,
        Some(ObservationRevision::new(1)?)
    );
    assert_eq!(cancel.value(), None);
    assert_eq!(cancel.times(), times()?);
    assert_eq!(original, original.clone());
    let (id, provider, transformation) = ctx.origin();
    let (date, codes) = ctx.session_conditions();
    assert_eq!(id.get(), 9);
    assert_eq!(codes, &[1, 65530]);
    for codes in [&[1, 1][..], &[2, 1][..], &[0; 17][..]] {
        assert_eq!(
            ObservationContext::new(id, provider, transformation, date, codes),
            Err(ObservationError::Conditions)
        );
    }
    let max_codes: Vec<u16> = (0..16).collect();
    assert_eq!(
        ObservationContext::new(id, provider, transformation, date, &max_codes)?
            .session_conditions()
            .1
            .len(),
        16
    );
    let text = format!("{original:?} {correction:?} {cancel:?}");
    assert!(!text.contains("65530"));
    assert!(text.contains("IndexPoints(..)"));
    Ok(())
}

#[test]
fn option_snapshots_pin_sides_ticks_and_point_in_time_specifications() -> TestResult {
    let reference = spec(0)?;
    let bid = QuoteSide::new(OptionPoints::new(0, 0)?, None, at(1, 1)?);
    let ask = QuoteSide::new(
        OptionPoints::new(5, 2)?,
        Some(DisplayedContracts::new(0)),
        at(1, 2)?,
    );
    let quote = OptionQuoteSnapshot::new(&reference, bid, ask)?;
    assert_eq!(quote.sides(), (bid, ask));
    assert_eq!(
        quote.specification(),
        (reference.id(), reference.revision())
    );
    assert_eq!(quote.sides().0.values().1, None);
    assert_eq!(
        quote.sides().1.values().1.map(DisplayedContracts::get),
        Some(0)
    );
    let ctx = context(
        1,
        contract(EvidenceKind::OptionSnapshot)?,
        DeliveryCapability::Snapshots,
    )?;
    let record = CanonicalObservation::new(
        ctx.clone(),
        version(1, None)?,
        times()?,
        None,
        Some(ObservationValue::OptionQuote(quote)),
    )?;
    assert_eq!(record.value(), Some(ObservationValue::OptionQuote(quote)));
    let positive = QuoteSide::new(
        OptionPoints::new(5, 2)?,
        Some(DisplayedContracts::new(u64::MAX)),
        at(1, 2)?,
    );
    assert_eq!(
        positive.values().1.map(DisplayedContracts::get),
        Some(u64::MAX)
    );
    assert!(OptionQuoteSnapshot::new(&reference, positive, positive).is_ok());
    assert_eq!(
        OptionQuoteSnapshot::new(&reference, positive, bid),
        Err(ObservationError::CrossedQuote)
    );
    let off_tick = QuoteSide::new(OptionPoints::new(1, 2)?, None, at(1, 0)?);
    assert_eq!(
        OptionQuoteSnapshot::new(&reference, off_tick, ask),
        Err(ObservationError::Tick)
    );
    for (reference, side_at, expected) in [
        (spec(3001)?, at(1, 0)?, ObservationError::Specification),
        (
            spec(0)?,
            at(-1, 999_999_999)?,
            ObservationError::Specification,
        ),
        (spec(0)?, at(2, 1)?, ObservationError::TimeOrder),
    ] {
        let side = QuoteSide::new(OptionPoints::new(0, 0)?, None, side_at);
        let quote = OptionQuoteSnapshot::new(&reference, side, side)?;
        assert_eq!(
            CanonicalObservation::new(
                ctx.clone(),
                version(1, None)?,
                times()?,
                None,
                Some(ObservationValue::OptionQuote(quote))
            ),
            Err(expected)
        );
    }
    let wrong_instrument = ObservationContext::new(
        InstrumentId::new(123)?,
        ctx.origin().1,
        TransformationRevision::new(1)?,
        SessionDate::new(1970, 1, 1)?,
        &[],
    )?;
    assert_eq!(
        CanonicalObservation::new(
            wrong_instrument,
            version(1, None)?,
            times()?,
            None,
            Some(ObservationValue::OptionQuote(quote))
        ),
        Err(ObservationError::Specification)
    );
    assert!(format!("{record:?}").contains("OptionPoints(..)"));
    assert_eq!(
        format!("{:?}", DisplayedContracts::new(987_654_321)),
        "DisplayedContracts(..)"
    );
    Ok(())
}

#[test]
fn declared_precision_is_checked_for_source_times_sides_and_bar_bounds() -> TestResult {
    let trade = ObservationValue::Trade(ShareTrade::new(
        UsdPrice::new(1, 0)?,
        ShareVolume::new(1, 0)?,
    )?);
    for (precision, nanos, accepted) in [
        (TimestampPrecision::Milliseconds, 0, true),
        (TimestampPrecision::Milliseconds, 1000, false),
        (TimestampPrecision::Microseconds, 1000, true),
        (TimestampPrecision::Microseconds, 1001, false),
        (TimestampPrecision::Nanoseconds, 1001, true),
    ] {
        let c = EvidenceContract::new(
            EvidenceKind::ShareTrade,
            MarketScope::Consolidated,
            semantics(1)?,
            precision,
        )?;
        let t = EvidenceTimes::new(at(1, nanos)?, None, None, at(2, 1)?)?;
        let result = CanonicalObservation::new(
            context(1, c, DeliveryCapability::Snapshots)?,
            version(1, None)?,
            t,
            None,
            Some(trade),
        );
        assert_eq!(result.is_ok(), accepted);
    }
    let coarse = EvidenceContract::new(
        EvidenceKind::RawBar(BarCoverage::Intraday),
        MarketScope::Consolidated,
        semantics(1)?,
        TimestampPrecision::Milliseconds,
    )?;
    let bounds = EvidenceInterval::new(at(0, 1)?, at(1, 0)?)?;
    let raw = RawBar::new(
        bounds,
        BarCoverage::Intraday,
        [UsdPrice::new(1, 0)?; 4],
        ShareVolume::new(0, 0)?,
    )?;
    let t = EvidenceTimes::new(at(2, 0)?, None, None, at(3, 1)?)?;
    assert_eq!(
        CanonicalObservation::new(
            context(1, coarse, DeliveryCapability::Snapshots)?,
            version(1, None)?,
            t,
            None,
            Some(ObservationValue::Bar(raw))
        ),
        Err(ObservationError::Timestamp)
    );
    let c = EvidenceContract::new(
        EvidenceKind::OptionSnapshot,
        MarketScope::Consolidated,
        semantics(1)?,
        TimestampPrecision::Milliseconds,
    )?;
    let side = QuoteSide::new(OptionPoints::new(0, 0)?, None, at(1, 1)?);
    let quote = OptionQuoteSnapshot::new(&spec(0)?, side, side)?;
    assert_eq!(
        CanonicalObservation::new(
            context(1, c, DeliveryCapability::Snapshots)?,
            version(1, None)?,
            t,
            None,
            Some(ObservationValue::OptionQuote(quote))
        ),
        Err(ObservationError::Timestamp)
    );
    send_sync::<CanonicalObservation>();
    send_sync::<ProviderCapability>();
    Ok(())
}

#[test]
fn synthetic_sources_conform_to_all_payload_shapes_without_erasing_provenance() -> TestResult {
    let side = QuoteSide::new(
        OptionPoints::new(987_654_321, 0)?,
        Some(DisplayedContracts::new(987_654_321)),
        at(1, 0)?,
    );
    let values = [
        ObservationValue::Bar(bar(BarCoverage::ProviderDay)?),
        ObservationValue::Trade(ShareTrade::new(
            UsdPrice::new(987_654_321, 0)?,
            ShareVolume::new(987_654_321, 0)?,
        )?),
        ObservationValue::IndexLevel(IndexPoints::new(987_654_321, 0)?),
        ObservationValue::OptionQuote(OptionQuoteSnapshot::new(&spec(0)?, side, side)?),
    ];
    let kinds = [
        EvidenceKind::RawBar(BarCoverage::ProviderDay),
        EvidenceKind::ShareTrade,
        EvidenceKind::IndexLevel,
        EvidenceKind::OptionSnapshot,
    ];
    for (value, kind) in values.into_iter().zip(kinds) {
        let a = CanonicalObservation::new(
            context(1, contract(kind)?, DeliveryCapability::Snapshots)?,
            version(1, None)?,
            times()?,
            None,
            Some(value),
        )?;
        let b = CanonicalObservation::new(
            context(2, contract(kind)?, DeliveryCapability::Snapshots)?,
            version(1, None)?,
            times()?,
            None,
            Some(value),
        )?;
        assert_eq!(a.value(), b.value());
        assert_eq!(a.times(), b.times());
        assert_eq!(
            a.context().origin().1.declaration(),
            b.context().origin().1.declaration()
        );
        assert_ne!(
            a.context().origin().1.source(),
            b.context().origin().1.source()
        );
        assert_ne!(a, b);
        assert!(!format!("{a:?} {b:?}").contains("987654321"));
    }
    for error in [
        ObservationError::Timestamp,
        ObservationError::TimeOrder,
        ObservationError::Ohlc,
        ObservationError::TradeSize,
        ObservationError::CrossedQuote,
        ObservationError::Tick,
        ObservationError::Specification,
        ObservationError::Contract,
        ObservationError::UnsupportedCapability,
        ObservationError::Revision,
        ObservationError::Sequence,
        ObservationError::Conditions,
    ] {
        assert!(!format!("{error} {error:?}").contains("987654321"));
        assert!(std::error::Error::source(&error).is_none());
    }
    Ok(())
}
