// Shared provider/account helpers are intentionally not all used by this pure target.
#[allow(dead_code)]
mod support;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use support::*;
#[path = "native_acceptance/fixture.rs"]
mod fixture;
use fixture::NativeFixture;

#[test]
fn native_acceptance_preserves_labels_receipts_values_and_complete_provenance() -> Result {
    let f = NativeFixture::new(1)?;
    let accepted = f.freeze(f.input())?;
    assert_eq!(accepted.data().capture, f.capture.data().clone());
    assert_eq!(accepted.data().lineage.len(), 2);
    assert_eq!(accepted.data().lineage[0].transformation.as_str(), "8");
    assert_eq!(accepted.data().quality.revision.as_str(), "7");
    assert_eq!(
        accepted
            .data()
            .references
            .iter()
            .filter(|r| r.kind == EvidenceReferenceKind::Calendar)
            .count(),
        3
    );
    assert_eq!(
        NativeDailyArtifact::decode(accepted.bytes(), accepted.digest())?,
        accepted
    );
    assert_eq!(
        f.freeze(f.input())?,
        accepted,
        "same inputs are byte deterministic"
    );
    assert!(matches!(
        DailyObject::decode(accepted.bytes(), accepted.digest())?,
        DailyObject::NativeAccepted(_)
    ));
    assert!(DailyEvidenceArtifact::decode(accepted.bytes(), accepted.digest()).is_err());
    assert!(DateOnlyDailyArtifact::decode(accepted.bytes(), accepted.digest()).is_err());
    let text = std::str::from_utf8(accepted.bytes())?;
    assert!(
        !text.contains("event_at")
            && !text.contains("published_at")
            && !text.contains("observed_at")
    );
    assert!(!format!("{accepted:?} {:?}", accepted.data()).contains("synthetic-2026a"));
    let mut precise_receipt = f.capture.data().clone();
    precise_receipt.rows[0].time.received_at.nanos = 123;
    precise_receipt.rows[0].time.derived_at.seconds += 1;
    let capture = DateOnlyDailyArtifact::new(precise_receipt)?;
    let mut input = f.input();
    input.capture = &capture;
    let admitted =
        f.f.catalogs()
            .admit(&f.f.request, now()?, &Cancellation::default())?;
    let later = freeze_native_evidence(
        &f.record,
        &admitted,
        f.f.catalogs(),
        at(NOW + 1000)?,
        &Cancellation::default(),
        input,
    )?;
    assert_eq!(later.data().capture.rows[0].time.received_at.nanos, 123);
    Ok(())
}
#[test]
fn native_acceptance_rejects_gaps_closed_days_and_changed_capture_bindings() -> Result {
    let f = NativeFixture::new(1)?;
    for mutation in 0..6 {
        let mut d = f.capture.data().clone();
        match mutation {
            0 => {
                d.rows.pop();
            }
            1 => d.scope.account = EvidenceId::new(&format!("{:032x}", 9))?,
            2 => d.selection_sha256 = DailyEvidenceArtifact::hash(b"different selection"),
            3 => d.rows[0].time.rule.source_sha256 = DailyEvidenceArtifact::hash(b"forged rule"),
            4 => {
                let original = d.rows[0].clone();
                d.rows[1] = original;
            }
            _ => d.rows[0].time.derived_at.seconds += 1,
        }
        match DateOnlyDailyArtifact::new(d) {
            Err(_) => {}
            Ok(capture) => {
                let mut input = f.input();
                input.capture = &capture;
                assert!(f.freeze(input).is_err(), "mutation {mutation}");
            }
        }
    }
    let empty = ReferenceHistory::new(vec![])?;
    let closed_date = SessionDate::new(1970, 1, 1)?;
    let missing_closed = ReferenceHistory::new(
        f.calendar
            .at(now()?)
            .filter(|c| c.value().key().1 != closed_date)
            .cloned()
            .collect(),
    )?;
    let mut incomplete = f.input();
    incomplete.references.calendar = &missing_closed;
    assert!(matches!(
        f.freeze(incomplete),
        Err(EvidenceFreezeError::Quality(DailyQualityError::Reference))
    ));
    let mut input = f.input();
    input.references.calendar = &empty;
    assert!(matches!(
        f.freeze(input),
        Err(EvidenceFreezeError::Quality(DailyQualityError::Reference))
    ));
    let mut input = f.input();
    let empty_mappings = ReferenceHistory::new(vec![])?;
    input.references.mappings = &empty_mappings;
    assert!(f.freeze(input).is_err());
    Ok(())
}
#[test]
fn native_acceptance_enforces_reference_freshness_actions_and_exact_outlier_thresholds() -> Result {
    fixture::reject_recent_bucket()?;
    let f = NativeFixture::new(1)?;
    let bad = version(
        60,
        1,
        at(0)?,
        CorporateAction::new(
            f.f.request.instrument(),
            SessionDate::new(1969, 12, 31)?,
            ActionKind::Unsupported,
        ),
    )?;
    let actions = ReferenceHistory::new(vec![bad])?;
    let mut input = f.input();
    input.references.actions = &actions;
    assert!(matches!(
        f.freeze(input),
        Err(EvidenceFreezeError::Quality(
            DailyQualityError::UnsupportedAction
        ))
    ));
    let closed = ReferenceHistory::new(vec![version(
        61,
        1,
        at(0)?,
        CorporateAction::new(
            f.f.request.instrument(),
            SessionDate::new(1970, 1, 1)?,
            ActionKind::CashDividend(UsdPrice::new(1, 2)?),
        ),
    )?])?;
    let mut input = f.input();
    input.references.actions = &closed;
    assert!(f.freeze(input).is_err());
    let future = version(
        50,
        1,
        at(NOW + 1)?,
        ActionCoverage::new(f.f.request.instrument(), f.f.request.range, at(NOW + DAY)?),
    )?;
    let expired = version(
        50,
        1,
        at(0)?,
        ActionCoverage::new(f.f.request.instrument(), f.f.request.range, now()?),
    )?;
    for coverage in [&future, &expired] {
        let mut input = f.input();
        input.references.action_coverage = coverage;
        assert!(f.freeze(input).is_err());
    }
    // 1.10 is exactly 1000 bps above 1.00; one smallest representable unit more fails.
    for (coefficient, scale, pass) in [(110, 2, true), (110000001, 8, false)] {
        let mut d = f.capture.data().clone();
        d.rows[1].ohlc = std::array::from_fn(|_| EvidenceDecimal {
            coefficient: coefficient.to_string(),
            scale,
        });
        // Canonical decimals must not retain trailing fractional zeros.
        if coefficient == 110 {
            d.rows[1].ohlc = std::array::from_fn(|_| EvidenceDecimal {
                coefficient: "11".into(),
                scale: 1,
            });
        }
        let capture = DateOnlyDailyArtifact::new(d)?;
        let mut input = f.input();
        input.capture = &capture;
        assert_eq!(f.freeze(input).is_ok(), pass);
    }
    let mut strict = NativeFixture::new(1)?;
    strict.policy = DailyQualityPolicy::new(TransformationRevision::new(7)?, 1, 1000, 100)?;
    let admitted =
        strict
            .f
            .catalogs()
            .admit(&strict.f.request, now()?, &Cancellation::default())?;
    strict.record = AdmissionRecord::new(
        &admitted,
        strict.record.limits(),
        ListingId::new(1)?,
        strict.policy,
    )?
    .with_configuration(strict.schedule.configuration_digest())?;
    // Repinning changes command identity, so use the new request-bound capture as well.
    let mut d = strict.capture.data().clone();
    d.scope.request_sha256 = strict.record.command().request_sha256.clone();
    strict.capture = DateOnlyDailyArtifact::new(d)?;
    assert!(matches!(
        strict.freeze(strict.input()),
        Err(EvidenceFreezeError::Quality(DailyQualityError::Reference))
    ));
    Ok(())
}
#[test]
fn native_acceptance_rechecks_authority_cancellation_and_original_admission() -> Result {
    let f = NativeFixture::new(1)?;
    let cancel = Cancellation::default();
    let admitted = f.f.catalogs().admit(&f.f.request, now()?, &cancel)?;
    cancel.cancel();
    assert!(
        freeze_native_evidence(
            &f.record,
            &admitted,
            f.f.catalogs(),
            now()?,
            &cancel,
            f.input()
        )
        .is_err()
    );
    let mut revoked = f.f.right;
    revoked.status = ReviewStatus::Revoked;
    let rights = ReferenceHistory::new(vec![version(1, 2, now()?, revoked)?])?;
    assert!(
        freeze_native_evidence(
            &f.record,
            &admitted,
            AuthorityCatalogs {
                rights: &rights,
                providers: &f.f.providers
            },
            now()?,
            &Cancellation::default(),
            f.input()
        )
        .is_err()
    );
    assert!(
        freeze_native_evidence(
            &f.record,
            &admitted,
            f.f.catalogs(),
            f.f.request.deadline,
            &Cancellation::default(),
            f.input()
        )
        .is_err()
    );
    let foreign = NativeFixture::new(2)?;
    assert!(
        freeze_native_evidence(
            &foreign.record,
            &admitted,
            f.f.catalogs(),
            now()?,
            &Cancellation::default(),
            f.input()
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn native_acceptance_requires_explicit_revision_chains_and_same_scoped_predecessor() -> Result {
    let f = NativeFixture::new(1)?;
    let previous = f.freeze(f.input())?;
    let mut next = NativeFixture::new(2)?;
    for v in &mut next.versions {
        *v = ObservationVersion::new(
            v.lineage().0,
            ObservationRevision::new(2)?,
            Some(ObservationRevision::new(1)?),
        )?;
    }
    assert!(
        next.freeze(next.input()).is_err(),
        "non-original rows need a retained predecessor"
    );
    let mut input = next.input();
    input.previous = Some(&previous);
    let corrected = next.freeze(input)?;
    assert_eq!(corrected.data().replaces.as_ref(), Some(previous.digest()));
    next.versions[1] = next.versions[0];
    let mut input = next.input();
    input.previous = Some(&previous);
    assert!(next.freeze(input).is_err());
    let mut d = previous.data().clone();
    d.capture.scope.audience = EvidenceId::new(&format!("{:032x}", 9))?;
    let foreign = NativeDailyArtifact::new(d)?;
    let other = NativeFixture::new(2)?;
    let mut input = other.input();
    input.previous = Some(&foreign);
    assert!(other.freeze(input).is_err());
    Ok(())
}
#[test]
fn native_artifact_decode_rejects_noncanonical_tampered_and_unaccepted_formats() -> Result {
    let f = NativeFixture::new(1)?;
    let accepted = f.freeze(f.input())?;
    for bytes in [
        vec![b' '; MAX_EVIDENCE_BYTES + 1],
        format!(" {}", std::str::from_utf8(accepted.bytes())?).into_bytes(),
        std::str::from_utf8(accepted.bytes())?
            .replacen(
                '{',
                "{\"version\":\"edgeagent.evidence.native-daily.v1\",",
                1,
            )
            .into_bytes(),
        f.capture.bytes().to_vec(),
    ] {
        assert!(NativeDailyArtifact::decode(&bytes, &DailyEvidenceArtifact::hash(&bytes)).is_err());
    }
    assert!(
        NativeDailyArtifact::decode(accepted.bytes(), &DailyEvidenceArtifact::hash(b"wrong"))
            .is_err()
    );
    for mutate in 0..5 {
        let mut d = accepted.data().clone();
        match mutate {
            0 => {
                d.references.pop();
            }
            1 => {
                d.lineage.pop();
            }
            2 => d.delete_by = d.captured_at,
            3 => d.lineage[0].predecessor = Some(EvidenceRevision::new("2")?),
            _ => d.capture.rows[0].time.derived_at.seconds = d.captured_at.seconds + 1,
        }
        assert!(NativeDailyArtifact::new(d).is_err());
    }
    Ok(())
}
