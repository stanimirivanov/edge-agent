use edgeagent_market_data_domain::*;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

// Entirely invented contract terms and dates; not exchange/product qualification.
fn utc(month: u32, day: u32, hour: u32, minute: u32) -> TestResult<UtcInstant> {
    let date = chrono::NaiveDate::from_ymd_opt(2026, month, day)
        .and_then(|d| d.and_hms_opt(hour, minute, 0))
        .ok_or("invalid fixture")?;
    Ok(UtcInstant::from_unix_millis(
        date.and_utc().timestamp_millis(),
    )?)
}
fn date(month: u32, day: u32) -> TestResult<SessionDate> {
    Ok(SessionDate::new(2026, month, day)?)
}
fn schedule(last: UtcInstant, expiry: UtcInstant) -> TestResult<OptionSchedule> {
    Ok(OptionSchedule::new(
        utc(1, 1, 0, 0)?,
        last,
        expiry,
        expiry,
        UtcInstant::from_unix_millis(expiry.unix_millis() + 172_800_000)?,
        OptionSchedule::timezone_rules().1,
    )?)
}
fn terms(exercise: ExerciseStyle, settlement: SettlementStyle) -> TestResult<OptionTerms> {
    Ok(OptionTerms::new(
        OptionKind::Call,
        OptionPoints::new(10025, 2)?,
        OptionPoints::new(5, 2)?,
        UsdPerPoint::new(100, 0)?,
        exercise,
        settlement,
    )?)
}
fn specification(
    underlying: Underlying,
    terms: OptionTerms,
    effective: EffectiveInterval,
) -> TestResult<OptionSpecification> {
    Ok(OptionSpecification::new(
        InstrumentId::new(10)?,
        underlying,
        terms,
        schedule(utc(3, 20, 20, 0)?, utc(3, 21, 3, 0)?)?,
        effective,
    )?)
}
fn version(
    revision: u64,
    known: UtcInstant,
    value: OptionSpecification,
) -> TestResult<ReferenceVersion<OptionSpecification>> {
    Ok(ReferenceVersion::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(revision)?,
        SourceId::new(1)?,
        known,
        [7; 32],
        value,
    ))
}

#[test]
fn exact_option_units_reject_invalid_values_without_rounding_or_share_coercion() -> TestResult {
    assert_eq!(OptionPoints::new(0, 8)?.decimal_parts(), (0, 0));
    assert_eq!(OptionPoints::new(-1, 0), Err(ValueError::InvalidSign));
    assert_eq!(OptionPoints::new(1, 9), Err(ValueError::Scale));
    assert_eq!(OptionPoints::new(i128::MAX, 0), Err(ValueError::Precision));
    assert_eq!(UsdPerPoint::new(0, 0), Err(ValueError::InvalidSign));
    assert_eq!(UsdPerPoint::new(-1, 0), Err(ValueError::InvalidSign));
    assert_eq!(UsdPerPoint::new(1, 9), Err(ValueError::Scale));
    assert_eq!(ContractQuantity::new(0), Err(ValueError::InvalidSign));
    assert_eq!(ContractQuantity::new(u64::MAX)?.get(), u64::MAX);
    let points = OptionPoints::new(1234500, 4)?;
    let (coefficient, scale) = points.decimal_parts();
    assert_eq!(OptionPoints::new(coefficient, scale)?, points);
    assert!(points.is_multiple_of(OptionPoints::new(5, 2)?));
    assert!(!OptionPoints::new(12346, 2)?.is_multiple_of(OptionPoints::new(5, 2)?));
    assert!(!points.is_multiple_of(OptionPoints::new(0, 0)?));
    assert!(OptionPoints::new(0, 0)?.is_multiple_of(OptionPoints::new(5, 2)?));
    assert_eq!(format!("{points:?}"), "OptionPoints(..)");
    assert_eq!(
        format!("{:?}", UsdPerPoint::new(1234567, 2)?),
        "UsdPerPoint(..)"
    );
    assert_eq!(
        format!("{:?}", ContractQuantity::new(1234567)?),
        "ContractQuantity(..)"
    );
    Ok(())
}

#[test]
fn checked_terms_model_all_styles_without_granting_initial_simulation_eligibility() -> TestResult {
    let deliverable = DeliverableReference::new(ReferenceId::new(42)?, ReferenceRevision::new(3)?);
    assert_eq!(
        deliverable.version(),
        (ReferenceId::new(42)?, ReferenceRevision::new(3)?)
    );
    let effective = EffectiveInterval::new(utc(3, 1, 0, 0)?, utc(3, 31, 0, 0)?)?;
    let id = InstrumentId::new(1)?;
    for exercise in [ExerciseStyle::European, ExerciseStyle::American] {
        for settlement in [
            SettlementStyle::Cash,
            SettlementStyle::Physical(deliverable),
        ] {
            for underlying in [
                Underlying::Index(id),
                Underlying::Equity(id),
                Underlying::Etf(id),
            ] {
                let terms = terms(exercise, settlement)?;
                let spec = specification(underlying, terms, effective)?;
                assert_eq!(spec.id(), InstrumentId::new(10)?);
                assert_eq!(spec.underlying(), underlying);
                assert_eq!(spec.effective(), effective);
                assert_eq!(spec.terms().lifecycle(), (exercise, settlement));
                let expected = if exercise == ExerciseStyle::European
                    && settlement == SettlementStyle::Cash
                    && matches!(underlying, Underlying::Index(_))
                {
                    Ok(())
                } else {
                    Err(DerivativeError::UnsupportedProfile)
                };
                assert_eq!(spec.validate_initial_simulation_shape(), expected);
            }
        }
    }
    let good = terms(ExerciseStyle::European, SettlementStyle::Cash)?;
    assert_eq!(
        good.payoff(),
        (OptionKind::Call, OptionPoints::new(10025, 2)?)
    );
    assert_eq!(
        good.quotation(),
        (
            OptionPoints::new(5, 2)?,
            UsdPerPoint::new(100, 0)?,
            Currency::Usd
        )
    );
    for (strike, tick) in [(0, 1), (1, 0)] {
        assert_eq!(
            OptionTerms::new(
                OptionKind::Put,
                OptionPoints::new(strike, 0)?,
                OptionPoints::new(tick, 0)?,
                UsdPerPoint::new(1, 0)?,
                ExerciseStyle::European,
                SettlementStyle::Cash
            ),
            Err(DerivativeError::InvalidTerms)
        );
    }
    assert_eq!(
        OptionSpecification::new(
            InstrumentId::new(10)?,
            Underlying::Index(InstrumentId::new(10)?),
            good,
            schedule(utc(3, 20, 20, 0)?, utc(3, 21, 3, 0)?)?,
            effective
        ),
        Err(DerivativeError::SelfReference)
    );
    // Strike and premium tick are distinct: a fractional strike is valid even
    // when it is not a multiple of the premium tick.
    assert!(
        OptionTerms::new(
            OptionKind::Put,
            OptionPoints::new(10003, 2)?,
            OptionPoints::new(5, 2)?,
            UsdPerPoint::new(10, 0)?,
            ExerciseStyle::American,
            SettlementStyle::Physical(deliverable)
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn explicit_deadlines_preserve_cutoffs_and_reject_contradictory_schedules() -> TestResult {
    let a = utc(3, 1, 0, 0)?;
    let l = utc(3, 19, 20, 0)?;
    let e = utc(3, 20, 13, 30)?;
    let x = utc(3, 20, 21, 0)?;
    let s = utc(3, 23, 13, 0)?;
    let revision = OptionSchedule::timezone_rules().1;
    let value = OptionSchedule::new(a, l, e, x, s, revision)?;
    assert_eq!(value.trading_bounds(), (a, l));
    assert_eq!(
        (
            value.expiration(),
            value.exercise_cutoff(),
            value.settlement()
        ),
        (e, x, s)
    );
    assert!(value.within_trading_lifetime(a));
    assert!(!value.within_trading_lifetime(UtcInstant::from_unix_millis(a.unix_millis() - 1)?));
    assert!(value.within_trading_lifetime(UtcInstant::from_unix_millis(l.unix_millis() - 1)?));
    assert!(!value.within_trading_lifetime(l));
    assert!(value.is_zero_dte_at(e)?);
    assert!(!value.within_trading_lifetime(e)); // AM-style expiration is not a last-trading session.
    for (a, l, e, x, s) in [
        (l, l, e, x, s),
        (a, x, e, x, s),
        (a, l, e, a, s),
        (a, l, e, x, l),
        (a, l, e, x, e),
    ] {
        assert_eq!(
            OptionSchedule::new(a, l, e, x, s, revision),
            Err(DerivativeError::Schedule)
        );
    }
    assert!(OptionSchedule::new(a, l, l, l, l, revision).is_ok());
    assert_eq!(
        OptionSchedule::new(a, l, e, x, s, "unknown-private-token"),
        Err(DerivativeError::TimezoneVersion)
    );
    let minimum = UtcInstant::from_unix_millis(-62_135_596_800_000)?;
    assert_eq!(
        OptionSchedule::new(minimum, l, e, x, s, revision),
        Err(DerivativeError::LocalDate)
    );
    assert_eq!(
        value.is_zero_dte_at(minimum),
        Err(DerivativeError::LocalDate)
    );
    assert!(!format!("{}", DerivativeError::TimezoneVersion).contains("private-token"));
    Ok(())
}

#[test]
fn local_expiration_and_zero_dte_survive_dst_and_utc_midnight() -> TestResult {
    assert_eq!(OptionSchedule::timezone_rules().0, "America/New_York");
    // Same local 23:00 expiration: winter UTC 04:00, summer UTC 03:00.
    for (month, day, last_hour, next_day, expiry_hour) in
        [(3, 6, 21, 7, 4), (3, 9, 20, 10, 3), (11, 2, 21, 3, 4)]
    {
        let schedule = schedule(
            utc(month, day, last_hour, 0)?,
            utc(month, next_day, expiry_hour, 0)?,
        )?;
        assert_eq!(schedule.expiration_date(), date(month, day)?);
        assert!(schedule.is_zero_dte_at(utc(month, next_day, 0, 30)?)?);
        assert!(!schedule.is_zero_dte_at(utc(month, day, 0, 30)?)?);
    }
    // Fall-back's two UTC instants map to the same local date; neither is guessed
    // from an ambiguous local wall time. Spring transition also preserves the date.
    for (month, day, first_hour, second_hour) in [(11, 1, 5, 6), (3, 8, 6, 7)] {
        let schedule = schedule(utc(month, day, 1, 0)?, utc(month, day, 9, 0)?)?;
        for hour in [first_hour, second_hour] {
            assert!(schedule.is_zero_dte_at(utc(month, day, hour, 30)?)?);
        }
    }
    let early = schedule(utc(11, 27, 18, 0)?, utc(11, 28, 4, 0)?)?;
    assert_eq!(early.trading_bounds().1, utc(11, 27, 18, 0)?);
    assert_eq!(early.expiration_date(), date(11, 27)?);
    Ok(())
}

#[test]
fn specification_revisions_use_effective_and_knowledge_boundaries_without_lookahead() -> TestResult
{
    let underlying = Underlying::Index(InstrumentId::new(1)?);
    let terms = terms(ExerciseStyle::European, SettlementStyle::Cash)?;
    let known = utc(3, 1, 0, 0)?;
    let corrected = utc(3, 16, 0, 0)?;
    let first = specification(
        underlying,
        terms,
        EffectiveInterval::new(utc(3, 1, 0, 0)?, utc(3, 31, 0, 0)?)?,
    )?;
    let correction = specification(
        underlying,
        terms,
        EffectiveInterval::new(utc(3, 15, 15, 30)?, utc(3, 31, 0, 0)?)?,
    )?;
    let original = version(1, known, first)?;
    let history =
        ReferenceHistory::new(vec![version(2, corrected, correction)?, original.clone()])?;
    let id = first.id();
    assert_eq!(
        history.resolve_option(id, utc(3, 10, 0, 0)?, known)?,
        &original
    );
    assert_eq!(
        history.resolve_option(id, utc(3, 10, 0, 0)?, corrected),
        Err(ReferenceError::Missing)
    );
    assert_eq!(
        history
            .resolve_option(id, utc(3, 15, 15, 30)?, corrected)?
            .revision(),
        ReferenceRevision::new(2)?
    );
    assert_eq!(
        history.resolve_option(id, utc(3, 31, 0, 0)?, corrected),
        Err(ReferenceError::Missing)
    );
    assert_eq!(
        history.resolve_option(
            id,
            utc(3, 1, 0, 0)?,
            UtcInstant::from_unix_millis(known.unix_millis() - 1)?
        ),
        Err(ReferenceError::Missing)
    );
    assert_eq!(
        history.resolve_option(InstrumentId::new(99)?, utc(3, 15, 15, 30)?, corrected),
        Err(ReferenceError::Missing)
    );
    let conflicting = ReferenceVersion::new(
        ReferenceId::new(2)?,
        ReferenceRevision::new(1)?,
        SourceId::new(2)?,
        known,
        [8; 32],
        first,
    );
    assert_eq!(
        ReferenceHistory::new(vec![original, conflicting])?.resolve_option(
            id,
            utc(3, 1, 0, 0)?,
            known
        ),
        Err(ReferenceError::Ambiguous)
    );
    fn portable<T: Send + Sync + Copy + Eq + std::fmt::Debug>() {}
    portable::<OptionSpecification>();
    portable::<ContractQuantity>();
    let (start, end) = correction.effective().bounds();
    assert_eq!(
        EffectiveInterval::new(start, start),
        Err(ReferenceError::InvalidInterval)
    );
    assert_eq!(
        EffectiveInterval::new(end, start),
        Err(ReferenceError::InvalidInterval)
    );
    assert_eq!(
        history.resolve_option(
            id,
            UtcInstant::from_unix_millis(start.unix_millis() - 1)?,
            corrected
        ),
        Err(ReferenceError::Missing)
    );
    assert!(!format!("{first:?}").contains("10025"));
    assert_eq!(first.schedule().expiration_date(), date(3, 20)?);
    Ok(())
}
