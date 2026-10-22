mod daily_support;
mod fixtures;
use daily_support::*;
use edgeagent_market_data_domain::*;
use fixtures::*;

#[test]
fn complete_explicit_calendars_preserve_dst_early_closes_and_raw_lineage() -> TestResult {
    let mut f = Fixture::new()?;
    f.calendar = ReferenceHistory::new(vec![
        version(
            1,
            1,
            f.known,
            day(3, 6, SessionHours::Regular(window(3, 6, 14, 21, 0)?))?,
        )?,
        version(2, 1, f.known, day(3, 7, SessionHours::Closed)?)?,
        version(3, 1, f.known, day(3, 8, SessionHours::Closed)?)?,
        version(
            4,
            1,
            f.known,
            day(3, 9, SessionHours::EarlyClose(window(3, 9, 13, 17, 0)?))?,
        )?,
    ])?;
    let checked = f.check(&f.bars, None)?;
    assert_eq!(checked.days().len(), 2);
    for (day, original) in checked.days().iter().zip(&f.bars) {
        assert_eq!(day.observation, original);
        assert_eq!(day.mapping.value().identities(), (f.instrument, f.listing));
        assert!(day.actions.is_empty());
    }
    assert_eq!(
        f.buckets[0].1.bounds().0,
        PreciseInstant::from_millis(utc(3, 6, 5, 0)?)
    );
    assert_eq!(
        f.buckets[1].1.bounds().0,
        PreciseInstant::from_millis(utc(3, 9, 4, 0)?)
    );
    assert_eq!(checked.provenance(), (f.policy, f.cutoff, &f.coverage));
    assert_eq!(f.policy.parameters().2, 1_000);
    assert_eq!(checked.range(), f.range);
    // An explicitly reviewed weekday closure removes an expectation; absence never does.
    drop(checked);
    f.calendar = ReferenceHistory::new(vec![
        version(1, 1, f.known, day(3, 6, SessionHours::Closed)?)?,
        version(2, 1, f.known, day(3, 7, SessionHours::Closed)?)?,
        version(3, 1, f.known, day(3, 8, SessionHours::Closed)?)?,
        version(
            4,
            1,
            f.known,
            day(3, 9, SessionHours::EarlyClose(window(3, 9, 13, 17, 0)?))?,
        )?,
    ])?;
    f.buckets.remove(0);
    f.bars.remove(0);
    assert_eq!(f.check(&f.bars, None)?.days().len(), 1);
    Ok(())
}

#[test]
fn missing_closed_conflicting_and_unbounded_series_fail_without_partial_acceptance() -> TestResult {
    let f = Fixture::new()?;
    assert_eq!(
        f.check(&f.bars[..1], None).err(),
        Some(DailyQualityError::Completeness)
    );
    let mut duplicate = f.bars.clone();
    duplicate.push(f.bar(3, 1, None, 0, 100, f.provider, None)?);
    assert_eq!(
        f.check(&duplicate, None).err(),
        Some(DailyQualityError::Completeness)
    );
    assert_eq!(
        f.check(&vec![f.bars[0].clone(); 4097], None).err(),
        Some(DailyQualityError::InvalidScope)
    );
    let mut spec = f.spec();
    spec.range = DateRange::new(date(1, 1)?, SessionDate::new(2027, 1, 3)?)?;
    assert_eq!(
        f.policy.check(&spec, f.references(), &f.bars, None).err(),
        Some(DailyQualityError::InvalidScope)
    );
    let oversized = vec![f.buckets[0]; 253];
    spec = f.spec();
    spec.buckets = &oversized;
    assert_eq!(
        f.policy.check(&spec, f.references(), &f.bars, None).err(),
        Some(DailyQualityError::InvalidScope)
    );
    let mut extra = Fixture::new()?;
    let closed_bucket = EvidenceInterval::new(
        PreciseInstant::from_millis(utc(3, 7, 5, 0)?),
        PreciseInstant::from_millis(utc(3, 8, 5, 0)?),
    )?;
    extra.buckets.push((date(3, 7)?, closed_bucket));
    extra
        .bars
        .push(extra.bar(3, 1, None, 2, 100, extra.provider, None)?);
    assert_eq!(
        extra.check(&extra.bars, None).err(),
        Some(DailyQualityError::Completeness)
    );
    Ok(())
}

#[test]
fn bucket_end_grace_and_reference_freshness_have_exact_inclusive_boundaries() -> TestResult {
    let mut f = Fixture::new()?;
    f.cutoff = UtcInstant::from_unix_millis(f.cutoff.unix_millis() - 1)?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::Bucket)
    );
    f.cutoff = utc(3, 10, 16, 0)?;
    assert!(f.check(&f.bars, None).is_ok());
    f.buckets[1].1 = EvidenceInterval::new(
        PreciseInstant::from_millis(utc(3, 9, 13, 30)?),
        PreciseInstant::from_millis(utc(3, 9, 20, 0)?),
    )?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::Bucket)
    );
    let mut f = Fixture::new()?;
    let age = u64::try_from(f.cutoff.unix_millis() - f.known.unix_millis())?;
    f.policy = DailyQualityPolicy::new(TransformationRevision::new(1)?, age, 1000, 100)?;
    assert!(f.check(&f.bars, None).is_ok());
    f.policy = DailyQualityPolicy::new(TransformationRevision::new(1)?, age - 1, 1000, 100)?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::Reference)
    );
    assert_eq!(
        DailyQualityPolicy::new(TransformationRevision::new(1)?, 0, 0, 0),
        Err(DailyQualityError::InvalidScope)
    );
    Ok(())
}

#[test]
fn revisions_duplicates_and_cancellations_select_before_effective_filtering() -> TestResult {
    let f = Fixture::new()?;
    let mut bars = f.bars.clone();
    bars.push(bars[0].clone());
    bars.reverse();
    assert_eq!(f.check(&bars, None)?.days()[0].observation, &f.bars[0]);
    bars.push(f.bar(1, 1, None, 0, 101, f.provider, None)?);
    assert_eq!(
        f.check(&bars, None).err(),
        Some(DailyQualityError::Revision)
    );
    let mut bars = f.bars.clone();
    let correction = f.bar(1, 2, Some(1), 0, 101, f.provider, Some(utc(3, 11, 0, 0)?))?;
    bars.push(correction);
    assert_eq!(f.check(&bars, None)?.days()[0].observation, &f.bars[0]);
    bars.pop();
    bars.push(f.bar(1, 2, Some(1), 0, 101, f.provider, Some(f.cutoff))?);
    assert_eq!(
        f.check(&bars, None)?.days()[0]
            .observation
            .version()
            .lineage()
            .1
            .get(),
        2
    );
    let changed = bars.pop().ok_or("missing fixture")?;
    bars.push(CanonicalObservation::new(
        changed.context().clone(),
        changed.version(),
        changed.times(),
        None,
        None,
    )?);
    assert_eq!(
        f.check(&bars, None).err(),
        Some(DailyQualityError::Completeness)
    );
    // A correction moving the effective date must not resurrect the old date.
    bars.pop();
    bars.push(f.bar(1, 2, Some(1), 1, 100, f.provider, Some(f.cutoff))?);
    assert_eq!(
        f.check(&bars, None).err(),
        Some(DailyQualityError::Completeness)
    );
    bars.remove(0);
    assert_eq!(
        f.check(&bars, None).err(),
        Some(DailyQualityError::Revision)
    );
    Ok(())
}

#[test]
fn action_coverage_and_point_in_time_reference_corrections_fail_closed() -> TestResult {
    let mut f = Fixture::new()?;
    f.coverage = version(
        1,
        1,
        utc(3, 11, 0, 0)?,
        ActionCoverage::new(f.instrument, f.range, utc(4, 1, 0, 0)?),
    )?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::Reference)
    );
    let mut f = Fixture::new()?;
    f.actions = ReferenceHistory::new(vec![version(
        1,
        1,
        f.known,
        CorporateAction::new(f.instrument, date(3, 7)?, ActionKind::Unsupported),
    )?])?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::UnsupportedAction)
    );
    let mut f = Fixture::new()?;
    let split = ActionKind::Split {
        before: std::num::NonZeroU64::MIN,
        after: std::num::NonZeroU64::new(2).ok_or("zero")?,
    };
    f.actions = ReferenceHistory::new(vec![
        version(
            1,
            1,
            f.known,
            CorporateAction::new(f.instrument, date(3, 9)?, split),
        )?,
        version(
            1,
            2,
            utc(3, 11, 0, 0)?,
            CorporateAction::new(f.instrument, date(3, 9)?, ActionKind::Unsupported),
        )?,
    ])?;
    assert_eq!(
        f.check(&f.bars, None)?.days()[1].actions[0].value().kind(),
        split
    );
    f.cutoff = utc(3, 11, 0, 0)?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::UnsupportedAction)
    );
    f.actions = ReferenceHistory::new(vec![])?;
    f.coverage = version(
        1,
        1,
        f.known,
        ActionCoverage::new(f.instrument, f.range, f.cutoff),
    )?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::Reference)
    );
    let mut f = Fixture::new()?;
    f.calendar = ReferenceHistory::new(vec![])?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::Reference)
    );
    let mut f = Fixture::new()?;
    f.mappings = ReferenceHistory::new(vec![
        version(1, 1, f.known, mapping(1, date(3, 1)?, date(4, 1)?)?)?,
        version(1, 2, f.cutoff, mapping(1, date(3, 8)?, date(4, 1)?)?)?,
    ])?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::Reference)
    );
    Ok(())
}

#[test]
fn exact_outlier_and_independent_source_thresholds_do_not_adjust_or_choose_prices() -> TestResult {
    let mut f = Fixture::new()?;
    f.bars[1] = f.bar(2, 1, None, 1, 110, f.provider, None)?;
    assert!(f.check(&f.bars, None).is_ok()); // exactly 10 percent
    f.bars[1] = replace_price(&f.bars[1], UsdPrice::new(11_000_000_001, 8)?)?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::Outlier)
    );
    f.bars[1] = f.bar(2, 1, None, 1, 111, f.provider, None)?;
    assert_eq!(
        f.check(&f.bars, None).err(),
        Some(DailyQualityError::Outlier)
    );
    let mut f = Fixture::new()?;
    let other = provider(2)?;
    let mut comparison = vec![
        f.bar(1, 1, None, 0, 101, other, None)?,
        f.bar(2, 1, None, 1, 100, other, None)?,
    ];
    assert!(f.check(&f.bars, Some((other, &comparison))).is_ok());
    comparison[0] = f.bar(1, 1, None, 0, 102, other, None)?;
    assert_eq!(
        f.check(&f.bars, Some((other, &comparison))).err(),
        Some(DailyQualityError::SourceDisagreement)
    );
    assert_eq!(
        f.check(&f.bars, Some((f.provider, &f.bars))).err(),
        Some(DailyQualityError::InvalidScope)
    );
    // Maximum supported coefficients and fractional scales compare exactly without overflow.
    let huge = UsdPrice::new(10_i128.pow(28) - 1, 0)?;
    for price in [huge, UsdPrice::new(1, 8)?] {
        for i in 0..2 {
            f.bars[i] = replace_price(&f.bars[i], price)?;
        }
        assert!(f.check(&f.bars, None).is_ok());
    }
    assert!(!format!("{:?}", f.check(&f.bars, None)?).contains("999999999999"));
    Ok(())
}
