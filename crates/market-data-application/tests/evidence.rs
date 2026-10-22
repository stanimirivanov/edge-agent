// The shared acquisition fixture also contains provider-port helpers unused by this target.
#[allow(dead_code)]
mod support;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use support::*;

fn produce(
    f: &Fixture,
    previous: Option<&DailyEvidenceArtifact>,
    omit_closed: bool,
) -> Result<std::result::Result<DailyEvidenceArtifact, EvidenceFreezeError>> {
    produce_with_rights(f, previous, omit_closed, &f.rights)
}
fn produce_with_rights(
    f: &Fixture,
    previous: Option<&DailyEvidenceArtifact>,
    omit_closed: bool,
    post_call_rights: &ReferenceHistory<RightsRecord>,
) -> Result<std::result::Result<DailyEvidenceArtifact, EvidenceFreezeError>> {
    let start = SessionDate::new(1970, 1, 1)?;
    let closed = SessionDate::new(1970, 1, 2)?;
    let symbol = Symbol::new("INVENTED")?;
    let mappings = ReferenceHistory::new(vec![version(
        1,
        1,
        at(0)?,
        InstrumentMapping::new(
            f.request.scope.instrument,
            ListingId::new(1)?,
            symbol.clone(),
            Venue::Xnys,
            f.request.range,
        ),
    )?])?;
    let mut days = vec![version(
        1,
        1,
        at(0)?,
        CalendarDay::new(
            Venue::Xnys,
            start,
            SessionHours::Regular(SessionWindow::new(
                at(14 * 3_600_000)?,
                at(21 * 3_600_000)?,
            )?),
            "synthetic",
        )?,
    )?];
    if !omit_closed {
        days.push(version(
            2,
            1,
            at(0)?,
            CalendarDay::new(Venue::Xnys, closed, SessionHours::Closed, "synthetic")?,
        )?);
    }
    let calendar = ReferenceHistory::new(days)?;
    let actions = ReferenceHistory::new(vec![])?;
    let coverage = version(
        1,
        1,
        at(0)?,
        ActionCoverage::new(f.request.scope.instrument, f.request.range, at(NOW + DAY)?),
    )?;
    let buckets = [(
        start,
        EvidenceInterval::new(
            PreciseInstant::from_millis(at(0)?),
            PreciseInstant::from_millis(at(DAY)?),
        )?,
    )];
    let series = DailySeriesSpec {
        instrument: f.request.scope.instrument,
        listing: ListingId::new(1)?,
        symbol: &symbol,
        venue: Venue::Xnys,
        range: f.request.range,
        cutoff: now()?,
        provider: f.review.capability,
        buckets: &buckets,
    };
    let command = AcquireEvidence {
        request: EvidenceId::new(&format!("{:032x}", f.request.id.get()))?,
        request_sha256: EvidenceDigest::new(&"2".repeat(64))?,
    };
    let mut batch = f.batch(now()?)?;
    for observation in &mut batch.observations {
        *observation = CanonicalObservation::new(
            observation.context().clone(),
            observation.version(),
            observation.times(),
            Some(u64::MAX),
            observation.value(),
        )?;
    }
    let cancel = Cancellation::default();
    let admitted = f.catalogs().admit(&f.request, now()?, &cancel)?;
    Ok(freeze_evidence(
        &admitted,
        AuthorityCatalogs {
            rights: post_call_rights,
            providers: &f.providers,
        },
        now()?,
        &cancel,
        EvidenceFreeze {
            command: &command,
            batch: &batch,
            series: &series,
            references: DailyReferences {
                mappings: &mappings,
                calendar: &calendar,
                actions: &actions,
                action_coverage: &coverage,
            },
            policy: DailyQualityPolicy::new(
                TransformationRevision::new(1)?,
                3 * DAY as u64,
                1000,
                100,
            )?,
            previous,
        },
    ))
}
fn fixture() -> Result<Fixture> {
    let mut f = Fixture::new()?;
    f.request.range = DateRange::new(SessionDate::new(1970, 1, 1)?, SessionDate::new(1970, 1, 3)?)?;
    Ok(f)
}

#[test]
fn freezing_preserves_exact_quality_lineage_and_closed_calendar_coverage() -> Result {
    let f = fixture()?;
    let a = produce(&f, None, false)??;
    assert_eq!(a, produce(&f, None, false)??);
    assert_eq!(DailyEvidenceArtifact::decode(a.bytes(), a.digest())?, a);
    assert_eq!(a.data().bars[0].ohlc[0].coefficient, "123456789");
    assert_eq!(a.data().bars[0].ohlc[0].scale, 4);
    assert_eq!(
        a.data().bars[0]
            .sequence
            .as_ref()
            .map(EvidenceSequence::as_str),
        Some("18446744073709551615")
    );
    assert_eq!(
        a.data()
            .references
            .iter()
            .filter(|r| r.kind == EvidenceReferenceKind::Calendar)
            .count(),
        2
    );
    assert_eq!(a.data().quality.reference_age_ms.as_str(), "259200000");
    assert_eq!(
        produce(&f, None, true)?.err(),
        Some(EvidenceFreezeError::Quality(DailyQualityError::Reference))
    );
    let command = AcquireEvidence {
        request: a.data().scope.request.clone(),
        request_sha256: a.data().scope.request_sha256.clone(),
    };
    assert!(EvidenceDecision::ready(command, &a).is_ok());
    let mut revoked = f.right;
    revoked.status = ReviewStatus::Revoked;
    let rights = ReferenceHistory::new(vec![version(1, 2, now()?, revoked)?])?;
    assert_eq!(
        produce_with_rights(&f, None, false, &rights)?.err(),
        Some(EvidenceFreezeError::Acquisition(AcquisitionError::Rights))
    );
    Ok(())
}

#[test]
fn corrections_link_immutable_predecessors_without_cross_scope_or_identity_reuse() -> Result {
    let mut f = fixture()?;
    let original = produce(&f, None, false)??;
    let bytes = original.bytes().to_vec();
    assert_eq!(
        produce(&f, Some(&original), false)?.err(),
        Some(EvidenceFreezeError::Contract)
    );
    f.request.id = AcquisitionId::new(2)?;
    let corrected = produce(&f, Some(&original), false)??;
    assert_eq!(corrected.data().replaces.as_ref(), Some(original.digest()));
    assert_ne!(original.digest(), corrected.digest());
    assert_eq!(original.bytes(), bytes);
    f.request.scope.audience = AudienceId::new(2)?;
    f.right.scope = f.request.scope;
    f.review.scope = f.request.scope;
    f.rights = ReferenceHistory::new(vec![version(1, 1, at(0)?, f.right)?])?;
    f.providers = ReferenceHistory::new(vec![version(1, 1, at(0)?, f.review)?])?;
    assert_eq!(
        produce(&f, Some(&original), false)?.err(),
        Some(EvidenceFreezeError::Contract)
    );
    f.right.status = ReviewStatus::Revoked;
    f.rights = ReferenceHistory::new(vec![version(1, 1, at(0)?, f.right)?])?;
    assert!(produce(&f, None, false).is_err());
    Ok(())
}
